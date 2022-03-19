import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  progressLogger,
} from '@pnpm/core-loggers'
import {
  Lockfile,
  PackageSnapshot,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import logger from '@pnpm/logger'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import packageIsInstallable from '@pnpm/package-is-installable'
import { Registries } from '@pnpm/types'
import {
  FetchPackageToStoreFunction,
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import * as dp from 'dependency-path'
import pathExists from 'path-exists'
import equals from 'ramda/src/equals'

const brokenModulesLogger = logger('_broken_node_modules')

export interface DependenciesGraphNode {
  alias?: string // this is populated in HoistedDepGraphOnly
  hasBundledDependencies: boolean
  modules: string
  name: string
  fetchingFiles: () => Promise<PackageFilesResponse>
  finishing: () => Promise<void>
  dir: string
  children: {[alias: string]: string}
  optionalDependencies: Set<string>
  optional: boolean
  depPath: string // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  isBuilt?: boolean
  requiresBuild: boolean
  prepare: boolean
  hasBin: boolean
  filesIndexFile: string
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export interface LockfileToDepGraphOptions {
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

export interface DirectDependenciesByImporterId {
  [importerId: string]: { [alias: string]: string }
}

export interface DepHierarchy {
  [depPath: string]: Record<string, DepHierarchy>
}

export interface LockfileToDepGraphResult {
  directDependenciesByImporterId: DirectDependenciesByImporterId
  graph: DependenciesGraph
  hierarchy?: DepHierarchy
  symlinkedDirectDependenciesByImporterId?: DirectDependenciesByImporterId
  prevGraph?: DependenciesGraph
  pkgLocationByDepPath?: Record<string, string>
}

export default async function lockfileToDepGraph (
  lockfile: Lockfile,
  currentLockfile: Lockfile | null,
  opts: LockfileToDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  const currentPackages = currentLockfile?.packages ?? {}
  const graph: DependenciesGraph = {}
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {}
  if (lockfile.packages != null) {
    const pkgSnapshotByLocation = {}
    await Promise.all(
      Object.keys(lockfile.packages).map(async (depPath) => {
        if (opts.skipped.has(depPath)) return
        const pkgSnapshot = lockfile.packages![depPath]
        // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
        const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        const modules = path.join(opts.virtualStoreDir, dp.depPathToFilename(depPath), 'node_modules')
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
        const dir = path.join(modules, pkgName)
        if (
          currentPackages[depPath] && equals(currentPackages[depPath].dependencies, lockfile.packages![depPath].dependencies) &&
          equals(currentPackages[depPath].optionalDependencies, lockfile.packages![depPath].optionalDependencies)
        ) {
          if (await pathExists(dir)) {
            return
          }

          brokenModulesLogger.debug({
            missing: dir,
          })
        }
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
        graph[dir] = {
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
        pkgSnapshotByLocation[dir] = pkgSnapshot
      })
    )
    const ctx = {
      force: opts.force,
      graph,
      lockfileDir: opts.lockfileDir,
      pkgSnapshotsByDepPaths: lockfile.packages,
      registries: opts.registries,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      skipped: opts.skipped,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
      virtualStoreDir: opts.virtualStoreDir,
    }
    for (const dir of Object.keys(graph)) {
      const pkgSnapshot = pkgSnapshotByLocation[dir]
      const allDeps = {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
      }

      const peerDeps = pkgSnapshot.peerDependencies ? new Set(Object.keys(pkgSnapshot.peerDependencies)) : null
      graph[dir].children = await getChildrenPaths(ctx, allDeps, peerDeps, '.')
    }
    for (const importerId of opts.importerIds) {
      const projectSnapshot = lockfile.importers[importerId]
      const rootDeps = {
        ...(opts.include.devDependencies ? projectSnapshot.devDependencies : {}),
        ...(opts.include.dependencies ? projectSnapshot.dependencies : {}),
        ...(opts.include.optionalDependencies ? projectSnapshot.optionalDependencies : {}),
      }
      directDependenciesByImporterId[importerId] = await getChildrenPaths(ctx, rootDeps, null, importerId)
    }
  }
  return { graph, directDependenciesByImporterId }
}

async function getChildrenPaths (
  ctx: {
    graph: DependenciesGraph
    force: boolean
    registries: Registries
    virtualStoreDir: string
    storeDir: string
    skipped: Set<string>
    pkgSnapshotsByDepPaths: Record<string, PackageSnapshot>
    lockfileDir: string
    sideEffectsCacheRead: boolean
    storeController: StoreController
  },
  allDeps: {[alias: string]: string},
  peerDeps: Set<string> | null,
  importerId: string
) {
  const children: {[alias: string]: string} = {}
  for (const alias of Object.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registries)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.lockfileDir, importerId, allDeps[alias].substr(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(allDeps[alias], alias) as string
    const childPkgSnapshot = ctx.pkgSnapshotsByDepPaths[childRelDepPath]
    if (ctx.graph[childRelDepPath]) {
      children[alias] = ctx.graph[childRelDepPath].dir
    } else if (childPkgSnapshot) {
      if (ctx.skipped.has(childRelDepPath)) continue
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      children[alias] = path.join(ctx.virtualStoreDir, dp.depPathToFilename(childRelDepPath), 'node_modules', pkgName)
    } else if (allDeps[alias].indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.lockfileDir, allDeps[alias].substr(5))
    } else if (!ctx.skipped.has(childRelDepPath) && ((peerDeps == null) || !peerDeps.has(alias))) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}
