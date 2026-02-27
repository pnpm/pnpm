import path from 'path'
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
import {
  type DepPath,
  type SupportedArchitectures,
  type Registries,
  type PkgIdWithPatchHash,
  type ProjectId,
  type AllowBuild,
} from '@pnpm/types'
import {
  type PkgRequestFetchResult,
  type FetchResponse,
  type StoreController,
} from '@pnpm/store-controller-types'
import * as dp from '@pnpm/dependency-path'
import { pathExists } from 'path-exists'
import { equals, isEmpty } from 'ramda'
import { iteratePkgsForVirtualStore } from './iteratePkgsForVirtualStore.js'

const brokenModulesLogger = logger('_broken_node_modules')

export interface DependenciesGraphNode {
  alias?: string // this is populated in HoistedDepGraphOnly
  hasBundledDependencies: boolean
  modules: string
  name: string
  version: string
  fetching?: () => Promise<PkgRequestFetchResult>
  forceImportPackage?: boolean // Used to force re-imports from the store of local tarballs that have changed.
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
  allowBuild?: AllowBuild
  autoInstallPeers: boolean
  enableGlobalVirtualStore?: boolean
  engineStrict: boolean
  force: boolean
  importerIds: ProjectId[]
  include: IncludedDependencies
  includeUnchangedDeps?: boolean
  ignoreScripts: boolean
  /**
   * When true, skip fetching local dependencies (file: protocol pointing to directories).
   * This is useful for `pnpm fetch` which only downloads packages from the registry
   * and doesn't need local packages that won't be available (e.g., in Docker builds).
   */
  ignoreLocalPackages?: boolean
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  patchedDependencies?: PatchGroupRecord
  registries: Registries
  sideEffectsCacheRead: boolean
  skipped: Set<DepPath>
  storeController: StoreController
  storeDir: string
  globalVirtualStoreDir: string
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
  injectionTargetsByDepPath: Map<string, string[]>
}

/**
 * Generate a dependency graph from lockfiles.
 *
 * If a current lockfile is provided, this function only includes new or changed
 * packages in the graph. In other words, the graph returned will be a set
 * subtraction of the packages in the wanted lockfile minus the current
 * lockfile. This behavior can be configured with the `includeUnchangedDeps`
 * option.
 */
export async function lockfileToDepGraph (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  const {
    graph,
    locationByDepPath,
    injectionTargetsByDepPath,
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

  return { graph, directDependenciesByImporterId, injectionTargetsByDepPath }
}

async function buildGraphFromPackages (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToDepGraphOptions
): Promise<{
    graph: DependenciesGraph
    locationByDepPath: Record<string, string>
    injectionTargetsByDepPath: Map<string, string[]>
  }> {
  const currentPackages = currentLockfile?.packages ?? {}
  const graph: DependenciesGraph = {}
  const locationByDepPath: Record<string, string> = {}
  // Only populated for directory deps (injected workspace packages)
  const injectionTargetsByDepPath = new Map<string, string[]>()

  const _getPatchInfo = getPatchInfo.bind(null, opts.patchedDependencies)
  const promises: Array<Promise<void>> = []
  const pkgSnapshotsWithLocations = iteratePkgsForVirtualStore(lockfile, opts)

  for (const { dirInVirtualStore, pkgMeta } of pkgSnapshotsWithLocations) {
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

      const isDirectoryDep = 'directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null
      if (isDirectoryDep && opts.ignoreLocalPackages) {
        logger.info({
          message: `Skipping local dependency ${pkgName}@${pkgVersion} (file: protocol)`,
          prefix: opts.lockfileDir,
        })
        return
      }

      const depIsPresent = !isDirectoryDep &&
        currentPackages[depPath] &&
        equals(currentPackages[depPath].dependencies, pkgSnapshot.dependencies)

      const depIntegrityIsUnchanged = isIntegrityEqual(pkgSnapshot.resolution, currentPackages[depPath]?.resolution)

      const modules = path.join(dirInVirtualStore, 'node_modules')
      const dir = path.join(modules, pkgName)
      locationByDepPath[depPath] = dir
      // Track directory deps for injected workspace packages
      if (isDirectoryDep) {
        injectionTargetsByDepPath.set(depPath, [dir])
      }

      let dirExists: boolean | undefined
      if (
        depIsPresent &&
        depIntegrityIsUnchanged &&
        isEmpty(currentPackages[depPath].optionalDependencies ?? {}) &&
        isEmpty(pkgSnapshot.optionalDependencies ?? {}) &&
        !opts.includeUnchangedDeps
      ) {
        dirExists = await pathExists(dir)
        if (dirExists) return
        brokenModulesLogger.debug({ missing: dir })
      }

      let fetchResponse!: Partial<FetchResponse>
      if (depIsPresent && depIntegrityIsUnchanged && equals(currentPackages[depPath].optionalDependencies, pkgSnapshot.optionalDependencies)) {
        if (dirExists ?? await pathExists(dir)) {
          fetchResponse = {}
        } else {
          brokenModulesLogger.debug({ missing: dir })
        }
      }

      if (!fetchResponse && opts.enableGlobalVirtualStore && !isDirectoryDep
        && !opts.force && !opts.includeUnchangedDeps) {
        if (dirExists ?? await pathExists(dir)) {
          fetchResponse = {}
        }
      }

      if (!fetchResponse) {
        const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
        progressLogger.debug({ packageId, requester: opts.lockfileDir, status: 'resolved' })

        try {
          fetchResponse = await opts.storeController.fetchPackage({
            allowBuild: opts.allowBuild,
            force: false,
            lockfileDir: opts.lockfileDir,
            ignoreScripts: opts.ignoreScripts,
            pkg: { name: pkgName, version: pkgVersion, id: packageId, resolution },
            supportedArchitectures: opts.supportedArchitectures,
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
        forceImportPackage: !depIntegrityIsUnchanged,
        hasBin: pkgSnapshot.hasBin === true,
        hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
        modules,
        name: pkgName,
        version: pkgVersion,
        optional: !!pkgSnapshot.optional,
        optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
        patch: _getPatchInfo(pkgName, pkgVersion),
      }
    })())
  }
  await Promise.all(promises)
  return { graph, locationByDepPath, injectionTargetsByDepPath }
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
    } else if (ref.startsWith('file:')) {
      children[alias] = path.resolve(ctx.lockfileDir, ref.slice(5))
    } else if (!ctx.skipped.has(childRelDepPath) && ((peerDeps == null) || !peerDeps.has(alias))) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}

function isIntegrityEqual (resolutionA?: LockfileResolution, resolutionB?: LockfileResolution) {
  // The LockfileResolution type is a union, but it doesn't have a "tag"
  // field to perform a discriminant match on. Using a type assertion is
  // required to get the integrity field.
  const integrityA = (resolutionA as ({ integrity?: string } | undefined))?.integrity
  const integrityB = (resolutionB as ({ integrity?: string } | undefined))?.integrity

  return integrityA === integrityB
}
