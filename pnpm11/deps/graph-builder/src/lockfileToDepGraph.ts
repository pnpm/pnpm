import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type {
  PkgRequestFetchResult,
  StoreController,
} from '@pnpm/store.controller-types'
import type { AllowBuild, DepPath, PkgIdWithPatchHash, ProjectId, RegistriesByScope, RegistryContext, SupportedArchitectures } from '@pnpm/types'

import { buildGraphFromPackages } from './buildGraphFromPackages.js'

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
  patch?: import('@pnpm/patching.types').PatchInfo
  resolution: import('@pnpm/lockfile.fs').LockfileResolution
  /**
   * Populated in the hoisted graph only. Maps the alias of each
   * `link:<root>/...` dependency to its path inside this package, where the
   * hoisted linker symlinks it from the package's own `node_modules`.
   */
  packageRootLinks?: Record<string, string>
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export interface LockfileToDepGraphOptions extends RegistryContext {
  allowBuild?: AllowBuild
  autoInstallPeers: boolean
  enableGlobalVirtualStore?: boolean
  engineStrict: boolean
  force: boolean
  /** See `installabilityUnderForce` in `@pnpm/config.package-is-installable`. */
  includeIncompatiblePackages?: boolean
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
  /**
   * Skip the `resolved` progress log for every package this graph fetches.
   * The default reporter counts each event without deduplicating by package,
   * so a caller that already ran a resolve pass over the same graph has
   * reported them and repeating them here inflates `Progress: resolved N`.
   */
  omitResolvedProgress?: boolean
  pnpmVersion: string
  patchedDependencies?: import('@pnpm/patching.config').PatchGroupRecord
  /**
   * The dep paths a non-optional edge reaches, as classified by
   * `filterLockfileByImportersAndEngine`. Installability is evaluated as
   * optional for everything outside this set.
   */
  requiredDepPaths: Set<DepPath>
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

  const childrenContext: GetChildrenPathsContext = {
    force: opts.force,
    graph,
    lockfileDir: opts.lockfileDir,
    registriesByScope: opts.registriesByScope,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    skipped: opts.skipped,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    locationByDepPath,
  }

  populateNodeChildren(graph, lockfile, opts.include, childrenContext)
  const directDependenciesByImporterId = resolveDirectDependencies(lockfile, opts, childrenContext)

  return { graph, directDependenciesByImporterId, injectionTargetsByDepPath }
}

function populateNodeChildren (
  graph: DependenciesGraph,
  lockfile: LockfileObject,
  include: IncludedDependencies,
  ctx: GetChildrenPathsContext
): void {
  for (const node of Object.values(graph)) {
    const pkgSnapshot = lockfile.packages![node.depPath]
    const allDeps = {
      ...pkgSnapshot.dependencies,
      ...(include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
    }
    const peerDeps = pkgSnapshot.peerDependencies ? new Set(Object.keys(pkgSnapshot.peerDependencies)) : null
    node.children = getChildrenPaths(ctx, allDeps, peerDeps, { importerId: '.', pkgDir: node.dir })
  }
}

function resolveDirectDependencies (
  lockfile: LockfileObject,
  opts: LockfileToDepGraphOptions,
  ctx: GetChildrenPathsContext
): DirectDependenciesByImporterId {
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {}
  for (const importerId of opts.importerIds) {
    const projectSnapshot = lockfile.importers[importerId]
    const rootDeps = {
      ...(opts.include.devDependencies ? projectSnapshot.devDependencies : {}),
      ...(opts.include.dependencies ? projectSnapshot.dependencies : {}),
      ...(opts.include.dependencies && opts.include.optionalDependencies ? projectSnapshot.optionalDependencies : {}),
    }
    directDependenciesByImporterId[importerId] = getChildrenPaths(ctx, rootDeps, null, { importerId })
  }
  return directDependenciesByImporterId
}

interface GetChildrenPathsContext {
  graph: DependenciesGraph
  force: boolean
  registriesByScope: RegistriesByScope
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
  parent: { importerId: string, pkgDir?: string }
): { [alias: string]: string } {
  const children: { [alias: string]: string } = {}
  for (const [alias, ref] of Object.entries(allDeps)) {
    const childPath = resolveChildPath(ctx, alias, ref, peerDeps, parent)
    if (childPath != null) children[alias] = childPath
  }
  return children
}

function resolveChildPath (
  ctx: GetChildrenPathsContext,
  alias: string,
  ref: string,
  peerDeps: Set<string> | null,
  parent: { importerId: string, pkgDir?: string }
): string | undefined {
  const packageRootLinkTarget = dp.packageRootLinkTarget(ref)
  if (packageRootLinkTarget != null && parent.pkgDir != null) {
    return path.join(parent.pkgDir, packageRootLinkTarget)
  }
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath === null) {
    return path.resolve(ctx.lockfileDir, parent.importerId, ref.slice(5))
  }
  return resolveDepPathLocation(ctx, childDepPath, ref, alias, peerDeps)
}

function resolveDepPathLocation (
  ctx: GetChildrenPathsContext,
  childRelDepPath: string,
  ref: string,
  alias: string,
  peerDeps: Set<string> | null
): string | undefined {
  if (ctx.locationByDepPath[childRelDepPath]) {
    return ctx.locationByDepPath[childRelDepPath]
  }
  if (ctx.graph[childRelDepPath]) {
    return ctx.graph[childRelDepPath].dir
  }
  if (ref.startsWith('file:')) {
    return path.resolve(ctx.lockfileDir, ref.slice(5))
  }
  if (!ctx.skipped.has(childRelDepPath as DepPath) && (peerDeps == null || !peerDeps.has(alias))) {
    throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
  }
  return undefined
}
