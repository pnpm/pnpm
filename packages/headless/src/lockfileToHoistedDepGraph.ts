import path from 'path'
import {
  progressLogger,
} from '@pnpm/core-loggers'
import {
  Lockfile,
  ProjectSnapshot,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import packageIsInstallable from '@pnpm/package-is-installable'
import { Registries } from '@pnpm/types'
import {
  FetchPackageToStoreFunction,
  StoreController,
} from '@pnpm/store-controller-types'
import hoist, { HoisterResult } from '@pnpm/real-hoist'
import {
  DependenciesGraph,
  DepHierarchy,
  DirectDependenciesByImporterId,
  LockfileToDepGraphResult,
} from './lockfileToDepGraph'

export interface LockfileToHoistedDepGraphOptions {
  engineStrict: boolean
  force: boolean
  importerIds: string[]
  include: IncludedDependencies
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
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
  const tree = hoist(lockfile)
  const graph: DependenciesGraph = {}
  const modulesDir = path.join(opts.lockfileDir, 'node_modules')
  let hierarchy = await fetchDeps(lockfile, opts, graph, modulesDir, tree.dependencies)
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {
    '.': directDepsMap(Object.keys(hierarchy), graph),
  }
  const symlinkedDirectDependenciesByImporterId: DirectDependenciesByImporterId = { '.': {} }
  for (const rootDep of Array.from(tree.dependencies)) {
    const reference = Array.from(rootDep.references)[0]
    if (reference.startsWith('workspace:')) {
      const importerId = reference.replace('workspace:', '')
      const modulesDir = path.join(opts.lockfileDir, importerId, 'node_modules')
      const nextHierarchy = (await fetchDeps(lockfile, opts, graph, modulesDir, rootDep.dependencies))
      hierarchy = {
        ...hierarchy,
        ...nextHierarchy,
      }

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
      directDeps[alias] = path.resolve(importerDir, ref.substr(5))
    }
  }
  return directDeps
}

async function fetchDeps (
  lockfile: Lockfile,
  opts: LockfileToHoistedDepGraphOptions,
  graph: DependenciesGraph,
  modules: string,
  deps: Set<HoisterResult>
): Promise<DepHierarchy> {
  const depHierarchy = {}
  await Promise.all(Array.from(deps).map(async (dep) => {
    const depPath = Array.from(dep.references)[0]
    if (opts.skipped.has(depPath) || depPath.startsWith('workspace:')) return
    const pkgSnapshot = lockfile.packages![depPath]
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
    progressLogger.debug({
      packageId,
      requester: opts.lockfileDir,
      status: 'resolved',
    })
    let fetchResponse!: ReturnType<FetchPackageToStoreFunction>
    try {
      fetchResponse = opts.storeController.fetchPackage({
        force: false,
        lockfileDir: opts.lockfileDir,
        pkg: {
          name: pkgName,
          version: pkgVersion,
          id: packageId,
          resolution,
        },
      })
      if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
    } catch (err: any) { // eslint-disable-line
      if (pkgSnapshot.optional) return
      throw err
    }
    fetchResponse.files() // eslint-disable-line
      .then(({ fromStore }) => {
        progressLogger.debug({
          packageId,
          requester: opts.lockfileDir,
          status: fromStore
            ? 'found_in_store'
            : 'fetched',
        })
      })
      .catch(() => {
        // ignore
      })
    graph[dir] = {
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
    }
    depHierarchy[dir] = await fetchDeps(lockfile, opts, graph, path.join(dir, 'node_modules'), dep.dependencies)
  }))
  return depHierarchy
}
