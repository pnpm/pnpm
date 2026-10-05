import path from 'node:path'

import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import * as dp from '@pnpm/deps.path'
import type { PackageSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { PkgResolutionId, PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { SupportedArchitectures } from '@pnpm/types'
import normalizePath from 'normalize-path'

import {
  isCurrentChildrenResolution,
  resolveMissingPeersFromCurrentChildrenResolution,
  setDependencyTreeNodeWithCurrentChildren,
  updateChildrenResolutionNodes,
} from './childrenResolution.js'
import { getDepsToResolve, getLockedDependenciesOfPkg } from './getDepsToResolve.js'
import { getNonDevWantedDependencies } from './getNonDevWantedDependencies.js'
import type { WantedDependency } from './getWantedDependencies.js'
import { type CollectedDependencies, collectResolvedDependencies, indexResolvedDependencies } from './indexResolvedDependencies.js'
import { getCatalogExistingVersionFromSnapshot, lookUpCatalogEntry } from './lookUpCatalogEntry.js'
import { filterMissingPeers, startResolvingPeers } from './missingPeers.js'
import type { NodeId } from './nextNodeId.js'
import { type DependencyUpdatePlan, getResolveDependencyOptions, planDependencyUpdate } from './planDependencyUpdate.js'
import type {
  ExtendedWantedDependency,
  LinkedDependency,
  ParentPkg,
  PeersResolutionResult,
  PkgAddress,
  PostponedPeersResolutionFunction,
  PostponedResolutionFunction,
  PostponedResolutionOpts,
  ResolutionContext,
  ResolvedDependenciesOptions,
  ResolvedDependenciesResult,
  ResolveDependenciesOfDependency,
  UpdateMatchingFunction,
} from './resolutionTypes.js'
import { resolveDependency } from './resolveDependency.js'

export { buildTree, claimChildrenResolution } from './childrenResolution.js'
export { getPinnedNameVer } from './getDepsToResolve.js'
export { collectMissingRequiredPeers } from './missingPeers.js'
export { getPkgsInfoFromIds } from './requestDependencyPackage.js'
export type * from './resolutionTypes.js'
export {
  detectNamedRegistryCollision,
  detectRegistryRevisionConflict,
  getManifestFromResponse,
} from './resolvedPackage.js'
export type { WantedDependency }

export async function resolveDependencies (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: ResolvedDependenciesOptions
): Promise<ResolvedDependenciesResult> {
  const {
    pkgAddresses,
    postponedResolutionsQueue,
    postponedPeersResolutionQueue,
  } = await resolveEachDependency(ctx, preferredVersions, wantedDependencies, options)
  const { currentParentPkgAliases, newPreferredVersions } = indexResolvedDependencies(ctx, pkgAddresses, {
    markUpdated: options.currentDepth === 0,
    preferredVersions,
    selectorWeight: 1,
  })
  const postponedResolutionOpts: PostponedResolutionOpts = {
    directDepVersions: options.directDepVersions,
    preferredVersions: newPreferredVersions,
    parentPkgAliases: {
      ...options.parentPkgAliases,
      ...currentParentPkgAliases,
    },
    publishedBy: options.publishedBy,
  }
  const childrenResults = await Promise.all(
    postponedResolutionsQueue.map((postponedResolution) => postponedResolution(postponedResolutionOpts))
  )
  if (!ctx.hoistPeers) {
    return { pkgAddresses, resolvingPeers: Promise.resolve({ missingPeers: {}, resolvedPeers: {} }) }
  }
  return {
    pkgAddresses,
    resolvingPeers: startResolvingPeers({
      childrenResults,
      pkgAddresses,
      parentPkgAliases: options.parentPkgAliases,
      currentParentPkgAliases,
      postponedPeersResolutionQueue,
      autoInstallPeersFromHighestMatch: ctx.autoInstallPeersFromHighestMatch,
    }),
  }
}

async function resolveEachDependency (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: ResolvedDependenciesOptions
): Promise<CollectedDependencies> {
  const extendedWantedDeps = getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    currentDepth: options.currentDepth,
    directDepVersions: options.directDepVersions,
    preferredDependencies: options.preferredDependencies,
    lockedDependencies: options.lockedDependencies,
    preferredVersions,
    prefix: options.prefix,
    proceed: options.proceed || ctx.forceFullResolution,
    ...pickRegistryContext(ctx),
    resolvedDependencies: options.resolvedDependencies,
    staleOverrideTargets: ctx.staleOverrideTargets,
  })
  // Resolve in parallel, then drain the results in input order.
  const resolvedDependencies = await Promise.all(
    extendedWantedDeps.map((extendedWantedDep) => resolveDependenciesOfDependency(
      ctx,
      preferredVersions,
      options,
      extendedWantedDep
    ))
  )
  return collectResolvedDependencies(resolvedDependencies)
}

export async function resolveDependenciesOfDependency (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  options: ResolvedDependenciesOptions,
  extendedWantedDep: ExtendedWantedDependency
): Promise<ResolveDependenciesOfDependency> {
  const plan = planDependencyUpdate(ctx, options, extendedWantedDep)
  const resolveDependencyOpts = getResolveDependencyOptions(ctx, { extendedWantedDep, options, plan, preferredVersions })
  replaceCatalogProtocolOfInjectedWorkspaceDep(ctx, options.parentPkg, extendedWantedDep)

  const resolveDependencyResult = await resolveDependency(extendedWantedDep.wantedDependency, ctx, resolveDependencyOpts)

  if (resolveDependencyResult == null) return { resolveDependencyResult: null }
  if (resolveDependencyResult.isLinkedDependency) {
    addLinkedDependencyNode(ctx, resolveDependencyResult)
    return { resolveDependencyResult }
  }
  if (!plan.update && extendedWantedDep.infoFromLockfile != null) {
    resolveDependencyResult.previousDepPath = extendedWantedDep.infoFromLockfile.depPath
    resolveDependencyResult.lockedPeerContext = extendedWantedDep.infoFromLockfile.lockedPeerContext
  }
  if (!resolveDependencyResult.isNew) {
    return {
      resolveDependencyResult,
      postponedPeersResolution: createPostponedPeersResolution(resolveDependencyResult),
    }
  }
  return {
    resolveDependencyResult,
    postponedResolution: createPostponedChildrenResolution(ctx, { extendedWantedDep, options, parentPkg: resolveDependencyResult, plan }),
  }
}

// The catalog protocol is normally replaced when resolving the dependencies
// of importers. However, when a workspace package is "injected", it becomes a
// "file:" dependency and is no longer an "importer" from the perspective of
// pnpm.
function replaceCatalogProtocolOfInjectedWorkspaceDep (
  ctx: ResolutionContext,
  parentPkg: ParentPkg,
  extendedWantedDep: ExtendedWantedDependency
): void {
  const isInjectedWorkspacePackage = parentPkg.resolvedVia === 'workspace' &&
    parentPkg.pkgId.startsWith('file:')
  if (!isInjectedWorkspacePackage) return
  const catalogLookup = lookUpCatalogEntry(ctx.catalogResolver, extendedWantedDep.wantedDependency)

  // The standard process for replacing the catalog protocol when resolving
  // the dependencies of "importers" stores the catalog lookup in the
  // dependency resolution result. This allows the catalogs snapshot section
  // of the wanted lockfile to be kept up to date.
  //
  // We can do a simple replacement here instead and discard the catalog
  // lookup object. It's not necessary to store this information for injected
  // workspace packages. The injected workspace package will still be resolved
  // as an importer separately, and we can rely on that process keeping the
  // importers lockfile catalog snapshots up to date.
  if (catalogLookup == null) return
  extendedWantedDep.wantedDependency.bareSpecifier = catalogLookup.specifier
  extendedWantedDep.preferredVersion = getCatalogExistingVersionFromSnapshot(catalogLookup, ctx.wantedLockfile, extendedWantedDep.wantedDependency)
}

function addLinkedDependencyNode (ctx: ResolutionContext, linkedDependency: LinkedDependency): void {
  const linkedNodeId = dp.packageRootLinkTarget(linkedDependency.pkgId) != null
    ? linkedDependency.pkgId as unknown as NodeId
    : createNodeIdForLinkedLocalPkg(ctx.lockfileDir, linkedDependency.resolution.directory)
  ctx.dependenciesTree.set(linkedNodeId, {
    children: {},
    depth: -1,
    installable: true,
    resolvedPackage: {
      name: linkedDependency.name,
      version: linkedDependency.version,
    },
  })
}

function createPostponedPeersResolution (pkgAddress: PkgAddress): PostponedPeersResolutionFunction | undefined {
  const { missingPeersOfChildren } = pkgAddress
  if (missingPeersOfChildren == null) return undefined
  return async (parentPkgAliases) => {
    const missingPeers = await missingPeersOfChildren.get()
    return filterMissingPeers({ missingPeers, resolvedPeers: {} }, parentPkgAliases)
  }
}

function createPostponedChildrenResolution (
  ctx: ResolutionContext,
  { extendedWantedDep, options, parentPkg, plan }: {
    extendedWantedDep: ExtendedWantedDependency
    options: ResolvedDependenciesOptions
    parentPkg: PkgAddress
    plan: DependencyUpdatePlan
  }
): PostponedResolutionFunction {
  const dependencyLockfile = extendedWantedDep.infoFromLockfile?.dependencyLockfile
  const resolveChildrenOfParent = resolveChildren.bind(null, ctx, {
    parentPkg,
    childrenResolutionId: parentPkg.childrenResolutionId!,
    dependencyLockfile,
    lockedDependencies: dependencyLockfile == null && !plan.updateRequested
      ? getLockedDependenciesOfPkg(ctx, parentPkg)
      : undefined,
    parentDepth: options.currentDepth,
    parentIds: [...options.parentIds, parentPkg.pkgId],
    updateDepth: plan.updateDepth,
    updatePatches: options.updatePatches,
    prefix: options.prefix,
    updateMatching: options.updateMatching,
    supportedArchitectures: options.supportedArchitectures,
    updateToLatest: options.updateToLatest,
  })
  return resolveChildrenWhileOwner.bind(null, ctx, {
    parentDepth: options.currentDepth,
    parentIds: options.parentIds,
    parentPkg,
    resolveChildrenOfParent,
  })
}

async function resolveChildrenWhileOwner (
  ctx: ResolutionContext,
  { parentDepth, parentIds, parentPkg, resolveChildrenOfParent }: {
    parentDepth: number
    parentIds: PkgResolutionId[]
    parentPkg: PkgAddress
    resolveChildrenOfParent: PostponedResolutionFunction
  },
  postponedResolutionOpts: PostponedResolutionOpts
): Promise<PeersResolutionResult> {
  if (!isCurrentChildrenResolution(ctx, parentPkg.pkgId, parentPkg.childrenResolutionId)) {
    setDependencyTreeNodeWithCurrentChildren(ctx, {
      parentDepth,
      parentIds: [...parentIds, parentPkg.pkgId],
      parentPkg,
    })
    return resolveMissingPeersFromCurrentChildrenResolution(ctx, parentPkg.pkgId, postponedResolutionOpts.parentPkgAliases)
  }
  const { missingPeers, resolvedPeers } = await resolveChildrenOfParent(postponedResolutionOpts)
  if (!isCurrentChildrenResolution(ctx, parentPkg.pkgId, parentPkg.childrenResolutionId)) {
    return resolveMissingPeersFromCurrentChildrenResolution(ctx, parentPkg.pkgId, postponedResolutionOpts.parentPkgAliases)
  }
  if (parentPkg.missingPeersOfChildren) {
    parentPkg.missingPeersOfChildren.resolved = true
    parentPkg.missingPeersOfChildren.resolve(missingPeers)
  }
  return filterMissingPeers({ missingPeers, resolvedPeers }, postponedResolutionOpts.parentPkgAliases)
}

export function createNodeIdForLinkedLocalPkg (lockfileDir: string, pkgDir: string): NodeId {
  return `link:${normalizePath(path.relative(lockfileDir, pkgDir))}` as NodeId
}

interface ParentOfChildren {
  parentPkg: PkgAddress
  childrenResolutionId: number
  parentIds: PkgResolutionId[]
  dependencyLockfile: PackageSnapshot | undefined
  lockedDependencies?: ResolvedDependencies
  parentDepth: number
  updateDepth: number
  updatePatches?: boolean
  prefix: string
  updateMatching?: UpdateMatchingFunction
  supportedArchitectures?: SupportedArchitectures
  updateToLatest?: boolean
}

async function resolveChildren (
  ctx: ResolutionContext,
  parent: ParentOfChildren,
  postponedResolutionOpts: PostponedResolutionOpts
): Promise<PeersResolutionResult> {
  const { parentPkg, childrenResolutionId, parentIds, parentDepth } = parent
  if (!isCurrentChildrenResolution(ctx, parentPkg.pkgId, childrenResolutionId)) {
    setDependencyTreeNodeWithCurrentChildren(ctx, {
      parentDepth,
      parentIds,
      parentPkg,
    })
    return {
      missingPeers: {},
      resolvedPeers: {},
    }
  }
  const childrenOptions = getChildrenResolutionOptions(parent, postponedResolutionOpts)
  const wantedDependencies = getNonDevWantedDependencies(parentPkg.pkg)
  const {
    pkgAddresses,
    resolvingPeers,
  } = await resolveDependencies(ctx, postponedResolutionOpts.preferredVersions, wantedDependencies, childrenOptions)
  if (!isCurrentChildrenResolution(ctx, parentPkg.pkgId, childrenResolutionId)) {
    setDependencyTreeNodeWithCurrentChildren(ctx, {
      parentDepth,
      parentIds,
      parentPkg,
    })
    return resolvingPeers
  }
  setResolvedChildren(ctx, parent, pkgAddresses)
  return resolvingPeers
}

function getChildrenResolutionOptions (
  { parentPkg, parentIds, dependencyLockfile, lockedDependencies, parentDepth, updateDepth, updatePatches, updateMatching, prefix, supportedArchitectures }: ParentOfChildren,
  { directDepVersions, parentPkgAliases, publishedBy }: PostponedResolutionOpts
): ResolvedDependenciesOptions {
  const currentResolvedDependencies = (dependencyLockfile != null)
    ? {
      ...dependencyLockfile.dependencies,
      ...dependencyLockfile.optionalDependencies,
    }
    : undefined
  const parentDependsOnPeer = Boolean(
    Object.keys(
      dependencyLockfile?.peerDependencies ??
      parentPkg.pkg.peerDependencies ??
      {}
    ).length
  )
  return {
    currentDepth: parentDepth + 1,
    directDepVersions,
    parentPkg,
    parentPkgAliases,
    preferredDependencies: currentResolvedDependencies,
    lockedDependencies,
    prefix,
    // If the package is not linked, we should also gather information about its dependencies.
    // After linking the package we'll need to symlink its dependencies.
    proceed: !parentPkg.depIsLinked || parentDependsOnPeer,
    publishedBy,
    resolvedDependencies: parentPkg.updated ? undefined : currentResolvedDependencies,
    updateDepth,
    updatePatches,
    updateMatching,
    supportedArchitectures,
    parentIds,
  }
}

function setResolvedChildren (
  ctx: ResolutionContext,
  { parentPkg, parentIds, parentDepth, dependencyLockfile }: ParentOfChildren,
  pkgAddresses: ResolvedDependenciesResult['pkgAddresses']
): void {
  ctx.childrenByParentId[parentPkg.pkgId] = pkgAddresses.map((child) => ({
    alias: child.alias,
    id: child.pkgId,
  }))
  ctx.dependenciesTree.set(parentPkg.nodeId, {
    children: pkgAddresses.reduce((chn, child) => {
      chn[child.alias] = (child as PkgAddress).nodeId ?? (child.pkgId as unknown as NodeId)
      return chn
    }, {} as Record<string, NodeId>),
    depth: parentDepth,
    installable: parentPkg.installable,
    dependencyNamesWhoseCurrentProviderMustWin: getDependencyNamesWhoseCurrentProviderMustWin(pkgAddresses, dependencyLockfile),
    lockedPeerContext: parentPkg.lockedPeerContext,
    previousDepPath: parentPkg.previousDepPath,
    resolvedPackage: ctx.resolvedPkgsById[parentPkg.pkgId],
  })
  ctx.nodeResolutionContextByNodeId.set(parentPkg.nodeId, {
    depth: parentDepth,
    installable: parentPkg.installable,
    parentIds,
    pkgId: parentPkg.pkgId,
  })
  updateChildrenResolutionNodes(ctx, parentPkg.pkgId, parentPkg.nodeId)
}

function getDependencyNamesWhoseCurrentProviderMustWin (
  pkgAddresses: ResolvedDependenciesResult['pkgAddresses'],
  dependencyLockfile: PackageSnapshot | undefined
): Set<string> {
  return new Set(pkgAddresses
    .filter((child) => {
      const previousRef = dependencyLockfile?.dependencies?.[child.alias] ??
        dependencyLockfile?.optionalDependencies?.[child.alias]
      if (previousRef == null || child.isLinkedDependency) return true
      return child.previousDepPath !== dp.refToRelative(previousRef, child.alias)
    })
    .map(({ alias }) => alias))
}
