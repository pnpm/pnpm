import pathExists from 'path-exists'
import path from 'path'
import {
  type Lockfile,
  type PackageSnapshot,
  type ProjectSnapshot,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { type PatchFile, type Registries } from '@pnpm/types'
import {
  type FetchPackageToStoreFunction,
  type StoreController,
} from '@pnpm/store-controller-types'
import { hoist, type HoistingLimits, type HoisterResult } from '@pnpm/real-hoist'
import * as dp from '@pnpm/dependency-path'
import {
  type DependenciesGraph,
  type DepHierarchy,
  type DirectDependenciesByImporterId,
  type LockfileToDepGraphResult,
} from '@pnpm/deps.graph-builder'

export interface LockfileToHoistedDepGraphOptions {
  autoInstallPeers: boolean
  engineStrict: boolean
  force: boolean
  hoistingLimits?: HoistingLimits
  externalDependencies?: Set<string>
  importerIds: string[]
  include: IncludedDependencies
  ignoreScripts: boolean
  currentHoistedLocations?: Record<string, string[]>
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

export async function lockfileToHoistedDepGraph (
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
  const tree = hoist(lockfile, {
    hoistingLimits: opts.hoistingLimits,
    externalDependencies: opts.externalDependencies,
    autoInstallPeers: opts.autoInstallPeers,
  })
  const graph: DependenciesGraph = {}
  const modulesDir = path.join(opts.lockfileDir, 'node_modules')
  const fetchDepsOpts = {
    ...opts,
    lockfile,
    graph,
    pkgLocationsByDepPath: {},
    hoistedLocations: {} as Record<string, string[]>,
  }
  const hierarchy = {
    [opts.lockfileDir]: await fetchDeps(fetchDepsOpts, modulesDir, tree.dependencies),
  }
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {
    '.': directDepsMap(Object.keys(hierarchy[opts.lockfileDir]), graph),
  }
  const symlinkedDirectDependenciesByImporterId: DirectDependenciesByImporterId = { '.': {} }
  await Promise.all(
    Array.from(tree.dependencies).map(async (rootDep) => {
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
    })
  )
  return {
    directDependenciesByImporterId,
    graph,
    hierarchy,
    pkgLocationsByDepPath: fetchDepsOpts.pkgLocationsByDepPath,
    symlinkedDirectDependenciesByImporterId,
    hoistedLocations: fetchDepsOpts.hoistedLocations,
  }
}

function directDepsMap (directDepDirs: string[], graph: DependenciesGraph): Record<string, string> {
  return directDepDirs.reduce((acc, dir) => {
    acc[graph[dir].alias!] = dir
    return acc
  }, {} as Record<string, string>)
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
  return Object.entries(rootDeps)
    .reduce((directDeps, [alias, ref]) => {
      if (ref.startsWith('link:')) {
        directDeps[alias] = path.resolve(importerDir, ref.slice(5))
      }
      return directDeps
    }, {} as Record<string, string>)
}

async function fetchDeps (
  opts: {
    graph: DependenciesGraph
    lockfile: Lockfile
    pkgLocationsByDepPath: Record<string, string[]>
    hoistedLocations: Record<string, string[]>
  } & LockfileToHoistedDepGraphOptions,
  modules: string,
  deps: Set<HoisterResult>
): Promise<DepHierarchy> {
  const depHierarchy: Record<string, DepHierarchy> = {}
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
    const depLocation = path.relative(opts.lockfileDir, dir)
    const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
    let fetchResponse!: ReturnType<FetchPackageToStoreFunction>
    // We check for the existence of the package inside node_modules.
    // It will only be missing if the user manually removed it.
    // That shouldn't normally happen but Bit CLI does remove node_modules in component directories:
    // https://github.com/teambit/bit/blob/5e1eed7cd122813ad5ea124df956ee89d661d770/scopes/dependencies/dependency-resolver/dependency-installer.ts#L169
    const skipFetch = opts.currentHoistedLocations?.[depPath]?.includes(depLocation) &&
      await pathExists(path.join(opts.lockfileDir, depLocation))
    const pkgResolution = {
      id: packageId,
      resolution,
    }
    if (skipFetch) {
      const { filesIndexFile } = opts.storeController.getFilesIndexFilePath({
        ignoreScripts: opts.ignoreScripts,
        pkg: pkgResolution,
      })
      fetchResponse = { filesIndexFile } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    } else {
      try {
        fetchResponse = opts.storeController.fetchPackage({
          force: false,
          lockfileDir: opts.lockfileDir,
          ignoreScripts: opts.ignoreScripts,
          pkg: pkgResolution,
          expectedPkg: {
            name: pkgName,
            version: pkgVersion,
          },
        }) as any // eslint-disable-line
        if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
      } catch (err: any) { // eslint-disable-line
        if (pkgSnapshot.optional) return
        throw err
      }
    }
    opts.graph[dir] = {
      alias: dep.name,
      children: {},
      depPath,
      dir,
      fetching: fetchResponse.fetching,
      filesIndexFile: fetchResponse.filesIndexFile,
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
    if (!opts.pkgLocationsByDepPath[depPath]) {
      opts.pkgLocationsByDepPath[depPath] = []
    }
    opts.pkgLocationsByDepPath[depPath].push(dir)
    depHierarchy[dir] = await fetchDeps(opts, path.join(dir, 'node_modules'), dep.dependencies)
    if (!opts.hoistedLocations[depPath]) {
      opts.hoistedLocations[depPath] = []
    }
    opts.hoistedLocations[depPath].push(depLocation)
    opts.graph[dir].children = getChildren(pkgSnapshot, opts.pkgLocationsByDepPath, opts)
  }))
  return depHierarchy
}

function getChildren (
  pkgSnapshot: PackageSnapshot,
  pkgLocationsByDepPath: Record<string, string[]>,
  opts: { include: IncludedDependencies }
) {
  const allDeps = {
    ...pkgSnapshot.dependencies,
    ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
  }
  const children: Record<string, string> = {}
  for (const [childName, childRef] of Object.entries(allDeps)) {
    const childDepPath = dp.refToRelative(childRef, childName)
    if (childDepPath && pkgLocationsByDepPath[childDepPath]) {
      children[childName] = pkgLocationsByDepPath[childDepPath][0]
    }
  }
  return children
}
