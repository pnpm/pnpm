import type { Catalogs } from '@pnpm/catalogs.types'
import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import { parse as parseDepPath } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { getPatchInfo, type PatchGroupRecord, verifyPatches } from '@pnpm/patching.config'
import type { ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type {
  DependenciesField,
  PeerDependencyIssuesByProjects,
  ProjectManifest,
  RangeSpecStyle,
} from '@pnpm/types'
import semver from 'semver'

import { extendGraph } from './extendGraph.js'
import { getCatalogSnapshots } from './getCatalogSnapshots.js'
import { getWantedDependencies, hasAlias, type ManifestWantedDependency, type WantedDependency } from './getWantedDependencies.js'
import type { ResolvedPkgsById } from './resolutionTypes.js'
import type { DependenciesTree, UpdateMatchingFunction } from './resolveDependencies.js'
import {
  type Importer,
  type LinkedDependency,
  type ResolveDependenciesOptions,
  resolveDependencyTree,
  type ResolveDependencyTreeResult,
  type ResolvedImporters,
  type ResolvedPackage,
} from './resolveDependencyTree.js'
import {
  type DependenciesByProjectId,
  type GenericDependenciesGraphNodeWithResolvedChildren,
  type GenericDependenciesGraphWithResolvedChildren,
  resolvePeers,
} from './resolvePeers.js'
import { type ProjectToLink, toProjectToLink } from './toProjectToLink.js'
import { type ResolveImporter, toResolveImporter } from './toResolveImporter.js'
import { updateLockfile } from './updateLockfile.js'
import { updateLockfileImporters } from './updateLockfileImporters.js'
import { wantedDepShouldUpdateCatalog } from './wantedDepShouldUpdateCatalog.js'

export type DependenciesGraph = GenericDependenciesGraphWithResolvedChildren<ResolvedPackage>

export type DependenciesGraphNode = GenericDependenciesGraphNodeWithResolvedChildren & ResolvedPackage

export {
  getWantedDependencies,
  hasAlias,
  type LinkedDependency,
  type ManifestWantedDependency,
  type RangeSpecStyle,
  type ResolvedPackage,
  type UpdateMatchingFunction,
  type WantedDependency,
}
export { isWorkspaceLocalPathSpecifier } from './updateProjectManifest.js'
export { assertValidDependencyAliases, isValidDependencyAlias } from './validateDependencyAlias.js'

export interface ImporterToResolve extends Importer<{
  nodeExecPath?: string
  rangeSpecStyle?: RangeSpecStyle
  updateSpec?: boolean
  preserveNonSemverVersionSpec?: boolean
}> {
  peer?: boolean
  peerAliases?: Set<string>
  rangeSpecStyle?: RangeSpecStyle
  binsDir: string
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  /**
   * Tells a declared range the update owns from one an override governs, so
   * `updateProjectManifest` leaves the latter where the project wrote it.
   * Built per project by `@pnpm/hooks.read-package-hook`.
   */
  isOverriddenDependency?: (alias: string, bareSpecifier: string) => boolean
  hookOwnedAliases?: Set<string>
  update?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest: boolean
  targetDependenciesField?: DependenciesField
}

export interface ResolveDependenciesResult {
  dependenciesByProjectId: DependenciesByProjectId
  dependenciesGraph: GenericDependenciesGraphWithResolvedChildren<ResolvedPackage>
  updatedCatalogs?: Catalogs | undefined
  outdatedDependencies: {
    [pkgId: string]: string
  }
  linkedDependenciesByProjectId: Record<string, LinkedDependency[]>
  newLockfile: LockfileObject
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  waitTillAllFetchingsFinish: () => Promise<void>
  wantedToBeSkippedPackageIds: Set<string>
  /**
   * Policy violations collected inline during resolution — each
   * resolver pushes to the list whenever it picks a version that
   * trips one of its own checks. Empty when no policy is active or no
   * pick violates.
   */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
}

export type ResolveDependenciesOpts = ResolveDependenciesOptions & {
  defaultUpdateDepth: number
  dedupePeerDependents?: boolean
  dedupePeers?: boolean
  dedupeDirectDeps?: boolean
  dedupeInjectedDeps?: boolean
  excludeLinksFromLockfile?: boolean
  preserveWorkspaceProtocol: boolean
  saveWorkspaceProtocol: 'rolling' | boolean
  lockfileIncludeTarballUrl?: boolean
  allowUnusedPatches?: boolean
  enableGlobalVirtualStore?: boolean
  allProjectIds: string[]
  /**
   * Generic checkpoint invoked between `resolveDependencyTree` and
   * `resolvePeers` once any inline-collected policy violations have
   * been gathered. Callers can prompt, persist, or throw based on
   * the violations. Throwing unwinds before any peer-dep work,
   * lockfile write, package.json update, or modules-dir change.
   * Intentionally policy-neutral: each resolver owns its violation
   * codes and the hook implementer (install command) decides what
   * to do with them.
   */
  handleResolutionPolicyViolations?: (violations: readonly ResolutionPolicyViolation[]) => Promise<void>
}

export async function resolveDependencies (
  importers: ImporterToResolve[],
  opts: ResolveDependenciesOpts
): Promise<ResolveDependenciesResult> {
  const projectsToResolve = await toResolveImporters(importers, opts)
  const resolution = await resolveDependencyTree(projectsToResolve, opts)
  await handleResolutionPolicyViolations(resolution.resolutionPolicyViolations, opts)

  opts.storeController.clearResolutionCache()

  const { resolvedImporters } = resolution
  const projectsToLink = await Promise.all(projectsToResolve.map(async (project) => toProjectToLink(project, {
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    lockfileDir: opts.lockfileDir,
    resolvedImporter: resolvedImporters[project.id],
  })))
  const {
    dependenciesGraph,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
  } = await resolvePeersOfProjects(projectsToLink, resolution, opts)

  const linkedDependenciesByProjectId = await updateLockfileImporters(projectsToResolve, {
    ...opts,
    dependenciesByProjectId,
    dependenciesGraph,
    importers,
    resolvedImporters,
  })
  const updatedCatalogs = getUpdatedCatalogs(projectsToResolve, resolvedImporters)

  if (opts.dedupeDirectDeps) {
    removeDirectDepsProvidedByRoot(dependenciesByProjectId)
  }

  await waitForResolutionFetches(resolution.resolvedPkgsById)

  const newLockfile = createNewLockfile({ dependenciesGraph, importersCount: importers.length, resolution, updatedCatalogs }, opts)

  return {
    dependenciesByProjectId,
    dependenciesGraph: extendGraph(dependenciesGraph, opts),
    outdatedDependencies: resolution.outdatedDependencies,
    linkedDependenciesByProjectId,
    updatedCatalogs,
    newLockfile,
    peerDependencyIssuesByProjects,
    waitTillAllFetchingsFinish: createWaitTillAllFetchingsFinish(resolution.resolvedPkgsById),
    wantedToBeSkippedPackageIds: resolution.wantedToBeSkippedPackageIds,
    resolutionPolicyViolations: resolution.resolutionPolicyViolations,
  }
}

async function toResolveImporters (
  importers: ImporterToResolve[],
  opts: ResolveDependenciesOpts
): Promise<ResolveImporter[]> {
  const _toResolveImporter = toResolveImporter.bind(null, {
    autoInstallPeers: opts.autoInstallPeers,
    defaultUpdateDepth: opts.defaultUpdateDepth,
    hideAlienModules: !opts.dryRun || opts.hideAlienModules === true,
    preferredVersions: opts.preferredVersions,
    preferredVersionsByImporterId: opts.preferredVersionsByImporterId,
    virtualStoreDir: opts.virtualStoreDir,
    globalVirtualStoreDir: opts.globalVirtualStoreDir,
    workspacePackages: opts.workspacePackages,
    noDependencySelectors: importers.every(({ wantedDependencies }) => wantedDependencies.length === 0),
  })
  return Promise.all(importers.map(async (project) => _toResolveImporter(project)))
}

/**
 * Resolver-policy gate between main resolution and peer-dep
 * resolution: every resolver records its own policy violations
 * inline as it picks each version, and we hand the accumulated
 * list to the install command's hook. The hook throws to abort
 * cleanly — nothing on disk has changed yet, and we haven't paid
 * the cost of peer resolution.
 *
 * If violations fired but no hook was wired, throw rather than
 * silently dropping them — the resolver-policy contract is "every
 * pick that trips a check produces a violation that gets handled";
 * a missing handler means the caller forgot to opt in and would
 * otherwise see policy-rejected versions land in the lockfile.
 */
async function handleResolutionPolicyViolations (
  resolutionPolicyViolations: ResolutionPolicyViolation[],
  opts: Pick<ResolveDependenciesOpts, 'handleResolutionPolicyViolations'>
): Promise<void> {
  if (resolutionPolicyViolations.length === 0) return
  if (!opts.handleResolutionPolicyViolations) {
    throw new PnpmError(
      'RESOLUTION_POLICY_VIOLATIONS_UNHANDLED',
      `${resolutionPolicyViolations.length} resolution-policy ${resolutionPolicyViolations.length === 1 ? 'violation was' : 'violations were'} produced but no handleResolutionPolicyViolations callback was wired to react to them.`,
      {
        hint: 'Internal: resolveDependencies needs a handleResolutionPolicyViolations callback whenever a policy that can produce violations (today: minimumReleaseAge) is active. Wire setupPolicyHandlers (in @pnpm/installing.commands) or supply a callback directly.',
      }
    )
  }
  await opts.handleResolutionPolicyViolations(resolutionPolicyViolations)
}

interface PeersOfProjects {
  dependenciesGraph: DependenciesGraph
  dependenciesByProjectId: DependenciesByProjectId
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
}

async function resolvePeersOfProjects (
  projectsToLink: ProjectToLink[],
  resolution: ResolveDependencyTreeResult,
  opts: ResolveDependenciesOpts
): Promise<PeersOfProjects> {
  const peerResolutionOpts = {
    allPeerDepNames: resolution.allPeerDepNames,
    dependenciesTree: resolution.dependenciesTree,
    dedupePeerDependents: opts.dedupePeerDependents,
    dedupePeers: opts.dedupePeers,
    dedupeInjectedDeps: opts.dedupeInjectedDeps,
    lockfileDir: opts.lockfileDir,
    projects: projectsToLink,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    resolvePeersFromWorkspaceRoot: Boolean(opts.resolvePeersFromWorkspaceRoot),
    resolvedImporters: resolution.resolvedImporters,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    workspaceProjectIds: new Set([...opts.allProjectIds, ...Object.keys(opts.wantedLockfile.importers)]),
  }
  const initiallyResolvedPeers = await resolvePeers(peerResolutionOpts)
  // A second pass reuses the peer contexts already recorded in the lockfile so a
  // writable install does not rewrite dependency instances whose locked provider
  // is still valid and present. It can only differ from the first pass for nodes
  // that carry a locked peer context, so it is skipped when none do (e.g. a fresh
  // install) to avoid resolving peers twice for no benefit.
  if (!treeHasLockedPeerContexts(resolution.dependenciesTree)) return initiallyResolvedPeers
  return resolvePeers({
    ...peerResolutionOpts,
    resolvedPeerProviderPaths: initiallyResolvedPeers.pathsByNodeId,
  })
}

function getUpdatedCatalogs (
  projectsToResolve: ResolveImporter[],
  resolvedImporters: ResolvedImporters
): Record<string, Record<string, string>> | undefined {
  let updatedCatalogs: Record<string, Record<string, string>> | undefined
  for (const project of projectsToResolve) {
    if (!project.updatePackageManifest) continue
    for (const { alias, catalogName, spec } of getCatalogUpdatesOfProject(project, resolvedImporters[project.id])) {
      updatedCatalogs ??= {}
      updatedCatalogs[catalogName] ??= {}
      updatedCatalogs[catalogName][alias] = spec
    }
  }
  return updatedCatalogs
}

function * getCatalogUpdatesOfProject (
  project: ResolveImporter,
  resolvedImporter: ResolvedImporters[string]
): Generator<{ alias: string, catalogName: string, spec: string }> {
  for (let index = 0; index < resolvedImporter.directDependencies.length; index++) {
    if (!wantedDepShouldUpdateCatalog(project.wantedDependencies[index])) continue
    const dep = resolvedImporter.directDependencies[index]
    if (dep.catalogLookup == null) continue
    // If normalizedBareSpecifier isn't defined, this catalog entry was resolved from cache.
    // Avoid updating the updatedCatalogs map since it is likely unchanged.
    if (dep.normalizedBareSpecifier == null) continue
    yield {
      alias: dep.alias,
      catalogName: dep.catalogLookup.catalogName,
      spec: isExplicitDistTagSpecifier(dep.wantedDependency?.bareSpecifier)
        ? dep.version
        : dep.normalizedBareSpecifier,
    }
  }
}

function removeDirectDepsProvidedByRoot (dependenciesByProjectId: DependenciesByProjectId): void {
  const rootDeps = dependenciesByProjectId['.']
  if (!rootDeps) return
  for (const [id, deps] of Object.entries(dependenciesByProjectId)) {
    if (id === '.') continue
    for (const [alias, depPath] of deps.entries()) {
      if (depPath === rootDeps.get(alias)) {
        deps.delete(alias)
      }
    }
  }
}

function createNewLockfile (
  {
    dependenciesGraph,
    importersCount,
    resolution,
    updatedCatalogs,
  }: {
    dependenciesGraph: DependenciesGraph
    importersCount: number
    resolution: ResolveDependencyTreeResult
    updatedCatalogs: Catalogs | undefined
  },
  opts: ResolveDependenciesOpts
): LockfileObject {
  const newLockfile = updateLockfile({
    dependenciesGraph,
    lockfile: opts.wantedLockfile,
    prefix: opts.virtualStoreDir,
    ...pickRegistryContext(opts),
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
  })
  if (resolution.time) {
    newLockfile.time = {
      ...opts.wantedLockfile.time,
      ...resolution.time,
    }
  }

  newLockfile.catalogs = getCatalogSnapshots(
    Object.values(resolution.resolvedImporters).flatMap(({ directDependencies }) => directDependencies),
    updatedCatalogs)

  if (
    opts.patchedDependencies &&
    Object.keys(opts.wantedLockfile.importers).length === importersCount
  ) {
    verifyPatches({
      patchedDependencies: opts.patchedDependencies,
      appliedPatches: getAppliedPatchKeys(newLockfile, opts.patchedDependencies),
      allowUnusedPatches: opts.allowUnusedPatches,
    })
  }
  return newLockfile
}

// waiting till package requests are finished
function createWaitTillAllFetchingsFinish (resolvedPkgsById: ResolvedPkgsById): () => Promise<void> {
  return async function waitTillAllFetchingsFinish (): Promise<void> {
    await Promise.all(Object.values(resolvedPkgsById).map(async ({ fetching }) => {
      try {
        await fetching?.()
      } catch {}
    }))
  }
}

function isExplicitDistTagSpecifier (bareSpecifier: string | undefined): boolean {
  return bareSpecifier != null && bareSpecifier !== 'latest' && !bareSpecifier.includes(':') && semver.validRange(bareSpecifier) == null
}

function treeHasLockedPeerContexts (dependenciesTree: DependenciesTree<ResolvedPackage>): boolean {
  for (const node of dependenciesTree.values()) {
    if (node.lockedPeerContext != null) return true
  }
  return false
}

function getAppliedPatchKeys (
  lockfile: LockfileObject,
  patchedDependencies: PatchGroupRecord
): Set<string> {
  const appliedPatchKeys = new Set<string>()
  for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {})) {
    if (!depPath.includes('(patch_hash=')) continue
    const { patchHash } = parseDepPath(depPath)
    if (patchHash == null) continue
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    if (version == null) continue
    const patch = getPatchInfo(patchedDependencies, name, version)
    if (patch != null && patchHash === `(patch_hash=${patch.hash})`) appliedPatchKeys.add(patch.key)
  }
  return appliedPatchKeys
}

/**
 * Waits for fetches that complete resolution data used by the lockfile snapshot and
 * virtual-store paths. Other package fetches are awaited later by `waitTillAllFetchingsFinish`.
 */
async function waitForResolutionFetches (resolvedPkgsById: Record<string, ResolvedPackage>): Promise<void> {
  const fetches: Array<Promise<unknown>> = []
  for (const pkg of Object.values(resolvedPkgsById)) {
    if (pkg.resolutionNeedsFetch && pkg.fetching != null) {
      fetches.push(pkg.fetching())
    }
  }
  if (fetches.length > 0) {
    await Promise.all(fetches)
  }
}
