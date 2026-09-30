import { parseDepPath } from '@pnpm/deps.path'
import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type { DepPath } from '@pnpm/types'
import * as semverUtils from '@yarnpkg/core/semverUtils'
import pDefer from 'p-defer'

import type { NodeId } from './nextNodeId.js'
import { toPkgByName } from './parentRefs.js'
import type {
  CurrentProviderSource,
  ParentRef,
  ParentRefs,
  PartialResolvedPackage,
  ResolveNodePeersContext,
} from './peersResolutionTypes.js'
import type { DependenciesTree, DependenciesTreeNode, PeerDependency } from './resolveDependencies.js'

interface CurrentProvidersContext<Pkg extends PartialResolvedPackage> {
  currentProviderSources: CurrentProviderSource[]
  parentNodeIds: NodeId[]
  dependenciesTree: DependenciesTree<Pkg>
}

export function getNodeIdsByPreviousDepPath<Pkg> (dependenciesTree: DependenciesTree<Pkg>): Map<DepPath, NodeId> {
  const nodeIdsByPreviousDepPath = new Map<DepPath, NodeId>()
  for (const [nodeId, node] of dependenciesTree.entries()) {
    if (node.previousDepPath != null && !nodeIdsByPreviousDepPath.has(node.previousDepPath)) {
      nodeIdsByPreviousDepPath.set(node.previousDepPath, nodeId)
    }
  }
  return nodeIdsByPreviousDepPath
}

export function pinLockedPeerProviders<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    node: DependenciesTreeNode<Pkg>
    resolvedPackage: Pkg
    parentPkgs: ParentRefs
    parentParentPkgs: ParentRefs
    parentNodeIds: NodeId[]
  }
): ParentRefs {
  let { parentPkgs } = opts
  if (opts.node.lockedPeerContext == null || ctx.resolvedPeerProviderPaths == null) return parentPkgs
  for (const [peerName, previousPeerDepPath] of Object.entries(opts.node.lockedPeerContext)) {
    const pinnable = getPinnableLockedPeer(ctx, {
      peerName,
      previousPeerDepPath,
      peerDependency: opts.resolvedPackage.peerDependencies[peerName],
      parentPkgs,
      parentNodeIds: opts.parentNodeIds,
    })
    if (pinnable == null) continue
    const peerPathPromise = ctx.pathsByNodeIdPromises.get(pinnable.peerNodeId) ?? pDefer<DepPath>()
    ctx.pathsByNodeIdPromises.set(pinnable.peerNodeId, peerPathPromise)
    ctx.pathsByNodeId.set(pinnable.peerNodeId, previousPeerDepPath)
    peerPathPromise.resolve(previousPeerDepPath)
    // Pinning writes into parentPkgs, and a childless node shares the object
    // with its parent, so siblings processed later would see the pinned
    // provider too. Copy before the first write to keep the pin scoped to
    // this node — sibling order follows resolution order, so a leak makes the
    // lockfile depend on network timing. Done here, not before the loop, so a
    // pass whose guards skip every entry never allocates.
    if (parentPkgs === opts.parentParentPkgs) {
      parentPkgs = { ...opts.parentParentPkgs }
    }
    parentPkgs[peerName] = pinnable.lockedPeer
  }
  return parentPkgs
}

function getPinnableLockedPeer<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    peerName: string
    previousPeerDepPath: DepPath
    peerDependency: PeerDependency | undefined
    parentPkgs: ParentRefs
    parentNodeIds: NodeId[]
  }
): { peerNodeId: NodeId, lockedPeer: ParentRef } | undefined {
  const { peerName, previousPeerDepPath, peerDependency } = opts
  const peerNodeId = ctx.nodeIdsByPreviousDepPath.get(previousPeerDepPath)
  if (peerNodeId == null || peerDependency == null) return undefined
  if (!lockedPeerProviderIsStable(ctx, peerNodeId, previousPeerDepPath)) return undefined
  if (hasCurrentPeerProviderThatMustWin(peerName, opts.parentPkgs, ctx)) return undefined
  const lockedPeer = toPkgByName([{
    alias: peerName,
    node: ctx.dependenciesTree.get(peerNodeId)!,
    nodeId: peerNodeId,
    parentNodeIds: opts.parentNodeIds,
  }])[peerName]
  if (!semverUtils.satisfiesWithPrereleases(lockedPeer.version, getPeerVersionRange(peerDependency.version), true)) return undefined
  return { peerNodeId, lockedPeer }
}

function lockedPeerProviderIsStable<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  peerNodeId: NodeId,
  previousPeerDepPath: DepPath
): boolean {
  if (ctx.resolvedPeerProviderPaths?.get(peerNodeId) !== previousPeerDepPath) return false
  // Only pin providers that have no peer context of their own. A provider
  // whose locked depPath carries a peer suffix is itself context-dependent
  // — and self-referential for cyclic peers — so reusing it can re-expand
  // cyclic suffixes and break the deterministic resolution that
  // https://github.com/pnpm/pnpm/issues/8155 established. Stable leaf
  // providers are the ones worth pinning; anything else falls back to
  // fresh resolution.
  if (parseDepPath(previousPeerDepPath).peerDepGraphHash !== '') return false
  // A provider that already resolved to a different path this pass no longer
  // matches its locked context; pinning it would leave pathsByNodeId
  // pointing at a depPath that was never added to the graph.
  const currentPeerDepPath = ctx.pathsByNodeId.get(peerNodeId)
  return currentPeerDepPath == null || currentPeerDepPath === previousPeerDepPath
}

export function hasCurrentPeerProviderThatMustWin<Pkg extends PartialResolvedPackage> (
  peerName: string,
  parentPkgs: ParentRefs,
  ctx: CurrentProvidersContext<Pkg>
): boolean {
  const peerNodeId = parentPkgs[peerName]?.nodeId
  if (peerNodeId == null) return false
  for (const source of ctx.currentProviderSources) {
    if (directDependencyProviderMustWin(ctx.dependenciesTree, source, { peerName, peerNodeId })) return true
  }
  return ancestorRequiresCurrentProvider(ctx, peerNodeId)
}

function directDependencyProviderMustWin<Pkg> (
  dependenciesTree: DependenciesTree<Pkg>,
  source: CurrentProviderSource,
  { peerName, peerNodeId }: { peerName: string, peerNodeId: NodeId }
): boolean {
  for (const [alias, directNodeId] of source.directNodeIdsByAlias) {
    if (directNodeId !== peerNodeId) continue
    if (
      alias !== peerName ||
      source.explicitlyRequestedDirectDependencies.has(alias) ||
      (source.declaredDirectDependencies.has(alias) &&
        dependenciesTree.get(peerNodeId)?.previousDepPath == null)
    ) return true
  }
  return false
}

function ancestorRequiresCurrentProvider<Pkg extends PartialResolvedPackage> (
  ctx: CurrentProvidersContext<Pkg>,
  peerNodeId: NodeId
): boolean {
  for (const parentNodeId of ctx.parentNodeIds) {
    const parentNode = ctx.dependenciesTree.get(parentNodeId)
    if (parentNode == null) continue
    const children = typeof parentNode.children === 'function' ? parentNode.children() : parentNode.children
    parentNode.children = children
    if ([...(parentNode.dependencyNamesWhoseCurrentProviderMustWin ?? [])].some((alias) =>
      children[alias] === peerNodeId
    )) return true
  }
  return false
}
