import path from 'path'
import {
  Lockfile,
  PackageSnapshot,
  ProjectSnapshot,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { PatchFile, Registries } from '@pnpm/types'
import {
  FetchPackageToStoreFunction,
  StoreController,
} from '@pnpm/store-controller-types'
import { hoist, HoistingLimits, HoisterResult } from '@pnpm/real-hoist'
import * as dp from 'dependency-path'
import {
  DependenciesGraph,
  DepHierarchy,
  DirectDependenciesByImporterId,
  LockfileToDepGraphResult,
} from './lockfileToDepGraph'

export interface LockfileToHoistedDepGraphOptions {
  engineStrict: boolean
  force: boolean
  hoistingLimits?: HoistingLimits
  importerIds: string[]
  include: IncludedDependencies
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
  patchedDependencies?: Record<string, PatchFile>
  sideEffectsCacheRead: boolean
  skipped: Set<string>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
}

export default async function lockfileToHoistedDepGraph (
  lockfile: Lockfile,
  currentLockfile: Lockfile | null,
  opts: LockfileToHoistedDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  let prevGraph!: DependenciesGraph
  if (currentLockfile?.packages != null) {
    prevGraph = (await _lockfileToHoistedDepGraph(currentLockfile, opts)).graph
  } else {
    prevGraph = {}
  }
  return {
    ...(await _lockfileToHoistedDepGraph(lockfile, opts)),
    prevGraph,
  }
}

async function _lockfileToHoistedDepGraph (
  lockfile: Lockfile,
  opts: LockfileToHoistedDepGraphOptions
): Promise<Omit<LockfileToDepGraphResult, 'prevGraph'>> {
  const tree = hoist(lockfile, { hoistingLimits: opts.hoistingLimits })
  const graph: DependenciesGraph = {}
  const modulesDir = path.join(opts.lockfileDir, 'node_modules')
  const fetchDepsOpts = {
    ...opts,
    lockfile,
    graph,
    pkgLocationByDepPath: {},
  }
  const hierarchy = {
    [opts.lockfileDir]: await fetchDeps(fetchDepsOpts, modulesDir, tree.dependencies),
  }
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {
    '.': directDepsMap(Object.keys(hierarchy[opts.lockfileDir]), graph),
  }
  const symlinkedDirectDependenciesByImporterId: DirectDependenciesByImporterId = { '.': {} }
  for (const rootDep of Array.from(tree.dependencies)) {
    const reference = Array.from(rootDep.references)[0]
    if (reference.startsWith('workspace:')) {
      const importerId = reference.replace('workspace:', '')
      const projectDir = path.join(opts.lockfileDir, importerId)
      const modulesDir = path.join(projectDir, 'node_modules')
      const nextHierarchy = (await fetchDeps(fetchDepsOpts, modulesDir, rootDep.dependencies))
      hierarchy[projectDir] = nextHierarchy

      const importer = lockfile.importers[importerId]
      const importerDir = path.join(opts.lockfileDir, importerId)
      symlinkedDirectDependenciesByImporterId[importerId] = pickLinkedDirectDeps(importer, importerDir, opts.include)
      directDependenciesByImporterId[importerId] = directDepsMap(Object.keys(nextHierarchy), graph)
    }
  }
  return {
    directDependenciesByImporterId,
    graph,
    hierarchy,
    pkgLocationByDepPath: fetchDepsOpts.pkgLocationByDepPath,
    symlinkedDirectDependenciesByImporterId,
  }
}

function directDepsMap (directDepDirs: string[], graph: DependenciesGraph): Record<string, string> {
  const result: Record<string, string> = {}
  for (const dir of directDepDirs) {
    result[graph[dir].alias!] = dir
  }
  return result
}

function pickLinkedDirectDeps (
  importer: ProjectSnapshot,
  importerDir: string,
  include: IncludedDependencies
): Record<string, string> {
  const rootDeps = {
    ...(include.devDependencies ? importer.devDependencies : {}),
    ...(include.dependencies ? importer.dependencies : {}),
    ...(include.optionalDependencies ? importer.optionalDependencies : {}),
  }
  const directDeps = {}
  for (const [alias, ref] of Object.entries(rootDeps)) {
    if (ref.startsWith('link:')) {
      directDeps[alias] = path.resolve(importerDir, ref.slice(5))
    }
  }
  return directDeps
}

async function fetchDeps (
  opts: {
    graph: DependenciesGraph
    lockfile: Lockfile
    pkgLocationByDepPath: Record<string, string>
  } & LockfileToHoistedDepGraphOptions,
  modules: string,
  deps: Set<HoisterResult>
): Promise<DepHierarchy> {
  const depHierarchy = {}
  await Promise.all(Array.from(deps).map(async (dep) => {
    const depPath = Array.from(dep.references)[0]
    if (opts.skipped.has(depPath) || depPath.startsWith('workspace:')) return
    const pkgSnapshot = opts.lockfile.packages![depPath]
    if (!pkgSnapshot) {
      // it is a link
      return
    }
    const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const packageId = packageIdFromSnapshot(depPath, pkgSnapshot, opts.registries)

    const pkg = {
      name: pkgName,
      version: pkgVersion,
      engines: pkgSnapshot.engines,
      cpu: pkgSnapshot.cpu,
      os: pkgSnapshot.os,
      libc: pkgSnapshot.libc,
    }
    if (!opts.force &&
      packageIsInstallable(packageId, pkg, {
        engineStrict: opts.engineStrict,
        lockfileDir: opts.lockfileDir,
        nodeVersion: opts.nodeVersion,
        optional: pkgSnapshot.optional === true,
        pnpmVersion: opts.pnpmVersion,
      }) === false
    ) {
      opts.skipped.add(depPath)
      return
    }
    const dir = path.join(modules, dep.name)
    const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
    let fetchResponse!: ReturnType<FetchPackageToStoreFunction>
    try {
      fetchResponse = opts.storeController.fetchPackage({
        force: false,
        lockfileDir: opts.lockfileDir,
        pkg: {
          id: packageId,
          resolution,
        },
        expectedPkg: {
          name: pkgName,
          version: pkgVersion,
        },
      })
      if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
    } catch (err: any) { // eslint-disable-line
      if (pkgSnapshot.optional) return
      throw err
    }
    opts.graph[dir] = {
      alias: dep.name,
      children: {},
      depPath,
      dir,
      fetchingFiles: fetchResponse.files,
      filesIndexFile: fetchResponse.filesIndexFile,
      finishing: fetchResponse.finishing,
      hasBin: pkgSnapshot.hasBin === true,
      hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
      modules,
      name: pkgName,
      optional: !!pkgSnapshot.optional,
      optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
      prepare: pkgSnapshot.prepare === true,
      requiresBuild: pkgSnapshot.requiresBuild === true,
      patchFile: opts.patchedDependencies?.[`${pkgName}@${pkgVersion}`],
    }
    opts.pkgLocationByDepPath[depPath] = dir
    depHierarchy[dir] = await fetchDeps(opts, path.join(dir, 'node_modules'), dep.dependencies)
    opts.graph[dir].children = getChildren(pkgSnapshot, opts.pkgLocationByDepPath, opts)
  }))
  return depHierarchy
}

function getChildren (
  pkgSnapshot: PackageSnapshot,
  pkgLocationByDepPath: Record<string, string>,
  opts: { include: IncludedDependencies }
) {
  const allDeps = {
    ...pkgSnapshot.dependencies,
    ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
  }
  const children = {}
  for (const [childName, childRef] of Object.entries(allDeps)) {
    const childDepPath = dp.refToRelative(childRef, childName)
    if (childDepPath && pkgLocationByDepPath[childDepPath]) {
      children[childName] = pkgLocationByDepPath[childDepPath]
    }
  }
  return children
}
