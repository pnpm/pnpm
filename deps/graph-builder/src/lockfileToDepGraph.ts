import path from 'node:path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  progressLogger,
} from '@pnpm/core-loggers'
import { type LockfileResolution, type LockfileObject } from '@pnpm/lockfile.fs'
import {
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { type PatchGroupRecord, getPatchInfo } from '@pnpm/patching.config'
import { type PatchInfo } from '@pnpm/patching.types'
import { type DepPath, type SupportedArchitectures, type Registries, type PkgIdWithPatchHash, type ProjectId } from '@pnpm/types'
import {
  type PkgRequestFetchResult,
  type FetchResponse,
  type StoreController,
} from '@pnpm/store-controller-types'
import * as dp from '@pnpm/dependency-path'
import pathExists from 'path-exists'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import { iteratePkgsForVirtualStore } from './iteratePkgsForVirtualStore'

const brokenModulesLogger = logger('_broken_node_modules')

export interface DependenciesGraphNode {
  alias?: string // this is populated in HoistedDepGraphOnly
  hasBundledDependencies: boolean
  modules: string
  name: string
  fetching?: () => Promise<PkgRequestFetchResult>
  dir: string
  children: Record<string, string>
  optionalDependencies: Set<string>
  optional: boolean
  depPath: DepPath // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  pkgIdWithPatchHash: PkgIdWithPatchHash
  isBuilt?: boolean
  requiresBuild?: boolean
  hasBin: boolean
  filesIndexFile?: string
  patch?: PatchInfo
  resolution: LockfileResolution
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export interface LockfileToDepGraphOptions {
  autoInstallPeers: boolean
  enableGlobalVirtualStore?: boolean
  engineStrict: boolean
  force: boolean
  importerIds: ProjectId[]
  include: IncludedDependencies
  includeUnchangedDeps?: boolean
  ignoreScripts: boolean
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  patchedDependencies?: PatchGroupRecord
  registries: Registries
  sideEffectsCacheRead: boolean
  skipped: Set<DepPath>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
  supportedArchitectures?: SupportedArchitectures
  virtualStoreDirMaxLength: number
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
  hoistedLocations?: Record<string, string[]>
  symlinkedDirectDependenciesByImporterId?: DirectDependenciesByImporterId
  prevGraph?: DependenciesGraph
  pkgLocationsByDepPath?: Record<string, string[]>
}

export async function lockfileToDepGraph (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  const {
    graph,
    locationByDepPath,
  } = await buildGraphFromPackages(lockfile, currentLockfile, opts)

  const _getChildrenPaths = getChildrenPaths.bind(null, {
    force: opts.force,
    graph,
    lockfileDir: opts.lockfileDir,
    registries: opts.registries,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    skipped: opts.skipped,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    locationByDepPath,
  } satisfies GetChildrenPathsContext)

  for (const node of Object.values(graph)) {
    const pkgSnapshot = lockfile.packages![node.depPath]
    const allDeps = {
      ...pkgSnapshot.dependencies,
      ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
    }
    const peerDeps = pkgSnapshot.peerDependencies ? new Set(Object.keys(pkgSnapshot.peerDependencies)) : null
    node.children = _getChildrenPaths(allDeps, peerDeps, '.')
  }

  const directDependenciesByImporterId: DirectDependenciesByImporterId = {}
  for (const importerId of opts.importerIds) {
    const projectSnapshot = lockfile.importers[importerId]
    const rootDeps = {
      ...(opts.include.devDependencies ? projectSnapshot.devDependencies : {}),
      ...(opts.include.dependencies ? projectSnapshot.dependencies : {}),
      ...(opts.include.optionalDependencies ? projectSnapshot.optionalDependencies : {}),
    }
    directDependenciesByImporterId[importerId] = _getChildrenPaths(rootDeps, null, importerId)
  }

  return { graph, directDependenciesByImporterId }
}

async function buildGraphFromPackages (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToDepGraphOptions
): Promise<{
    graph: DependenciesGraph
    locationByDepPath: Record<string, string>
  }> {
  const currentPackages = currentLockfile?.packages ?? {}
  const graph: DependenciesGraph = {}
  const locationByDepPath: Record<string, string> = {}

  const _getPatchInfo = getPatchInfo.bind(null, opts.patchedDependencies)
  const promises: Array<Promise<void>> = []
  const pkgSnapshotsWithLocations = iteratePkgsForVirtualStore(lockfile, opts)

  for (const { dirNameInVirtualStore, pkgMeta } of pkgSnapshotsWithLocations) {
    promises.push((async () => {
      const { pkgIdWithPatchHash, name: pkgName, version: pkgVersion, depPath, pkgSnapshot } = pkgMeta
      if (opts.skipped.has(depPath)) return

      const pkg = {
        name: pkgName,
        version: pkgVersion,
        engines: pkgSnapshot.engines,
        cpu: pkgSnapshot.cpu,
        os: pkgSnapshot.os,
        libc: pkgSnapshot.libc,
      }

      const packageId = packageIdFromSnapshot(depPath, pkgSnapshot)
      if (!opts.force && packageIsInstallable(packageId, pkg, {
        engineStrict: opts.engineStrict,
        lockfileDir: opts.lockfileDir,
        nodeVersion: opts.nodeVersion,
        optional: pkgSnapshot.optional === true,
        supportedArchitectures: opts.supportedArchitectures,
      }) === false) {
        opts.skipped.add(depPath)
        return
      }

      const depIsPresent = !('directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null) &&
        currentPackages[depPath] &&
        equals(currentPackages[depPath].dependencies, pkgSnapshot.dependencies)

      const modules = path.join(opts.virtualStoreDir, dirNameInVirtualStore, 'node_modules')
      const dir = path.join(modules, pkgName)
      locationByDepPath[depPath] = dir

      let dirExists: boolean | undefined
      if (
        depIsPresent &&
        isEmpty(currentPackages[depPath].optionalDependencies ?? {}) &&
        isEmpty(pkgSnapshot.optionalDependencies ?? {}) &&
        !opts.includeUnchangedDeps
      ) {
        dirExists = await pathExists(dir)
        if (dirExists) return
        brokenModulesLogger.debug({ missing: dir })
      }

      let fetchResponse!: Partial<FetchResponse>
      if (depIsPresent && equals(currentPackages[depPath].optionalDependencies, pkgSnapshot.optionalDependencies)) {
        if (dirExists ?? await pathExists(dir)) {
          fetchResponse = {}
        } else {
          brokenModulesLogger.debug({ missing: dir })
        }
      }

      if (!fetchResponse) {
        const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
        progressLogger.debug({ packageId, requester: opts.lockfileDir, status: 'resolved' })

        try {
          fetchResponse = await opts.storeController.fetchPackage({
            force: false,
            lockfileDir: opts.lockfileDir,
            ignoreScripts: opts.ignoreScripts,
            pkg: { id: packageId, resolution },
            expectedPkg: { name: pkgName, version: pkgVersion },
          })
        } catch (err) {
          if (pkgSnapshot.optional) return
          throw err
        }
      }

      graph[dir] = {
        children: {},
        pkgIdWithPatchHash,
        resolution: pkgSnapshot.resolution,
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
        patch: _getPatchInfo(pkgName, pkgVersion),
      }
    })())
  }
  await Promise.all(promises)
  return { graph, locationByDepPath }
}

interface GetChildrenPathsContext {
  graph: DependenciesGraph
  force: boolean
  registries: Registries
  virtualStoreDir: string
  storeDir: string
  skipped: Set<DepPath>
  lockfileDir: string
  sideEffectsCacheRead: boolean
  storeController: StoreController
  locationByDepPath: Record<string, string>
  virtualStoreDirMaxLength: number
}

function getChildrenPaths (
  ctx: GetChildrenPathsContext,
  allDeps: { [alias: string]: string },
  peerDeps: Set<string> | null,
  importerId: string
): { [alias: string]: string } {
  const children: { [alias: string]: string } = {}
  for (const [alias, ref] of Object.entries(allDeps)) {
    const childDepPath = dp.refToRelative(ref, alias)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.lockfileDir, importerId, ref.slice(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(ref, alias)!
    if (ctx.locationByDepPath[childRelDepPath]) {
      children[alias] = ctx.locationByDepPath[childRelDepPath]
    } else if (ctx.graph[childRelDepPath]) {
      children[alias] = ctx.graph[childRelDepPath].dir
    } else if (ref.indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.lockfileDir, ref.slice(5))
    } else if (!ctx.skipped.has(childRelDepPath) && ((peerDeps == null) || !peerDeps.has(alias))) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}
