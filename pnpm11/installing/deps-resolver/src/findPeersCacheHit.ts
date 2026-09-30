import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'

import type { NodeId } from './nextNodeId.js'
import type {
  ParentPkgInfo,
  ParentPkgsOfNode,
  ParentRefs,
  PartialResolvedPackage,
  PeersCache,
  PeersCacheItem,
} from './peersResolutionTypes.js'
import type { DependenciesTree } from './resolveDependencies.js'

interface ParentPackagesContext {
  parentPkgsOfNode: ParentPkgsOfNode
  purePkgs: Set<PkgIdWithPatchHash>
}

interface PeersCacheContext<Pkg extends PartialResolvedPackage> extends ParentPackagesContext {
  peersCache: PeersCache
  pathsByNodeId: Map<NodeId, DepPath>
  dependenciesTree: DependenciesTree<Pkg>
}

export function findHit<Pkg extends PartialResolvedPackage> (
  ctx: PeersCacheContext<Pkg>,
  parentPkgs: ParentRefs,
  pkgIdWithPatchHash: PkgIdWithPatchHash
): PeersCacheItem | undefined {
  const cacheItems = ctx.peersCache.get(pkgIdWithPatchHash)
  if (!cacheItems) return undefined
  return cacheItems.find((cache) => cacheItemMatchesParentPkgs(ctx, parentPkgs, cache))
}

function cacheItemMatchesParentPkgs<Pkg extends PartialResolvedPackage> (
  ctx: PeersCacheContext<Pkg>,
  parentPkgs: ParentRefs,
  cache: PeersCacheItem
): boolean {
  for (const [name, cachedNodeId] of cache.resolvedPeers) {
    if (!cachedPeerMatches(ctx, parentPkgs[name]?.nodeId, cachedNodeId)) return false
  }
  for (const missingPeer of cache.missingPeers.keys()) {
    if (parentPkgs[missingPeer]) return false
  }
  return true
}

function cachedPeerMatches<Pkg extends PartialResolvedPackage> (
  ctx: PeersCacheContext<Pkg>,
  parentPkgNodeId: NodeId | undefined,
  cachedNodeId: NodeId
): boolean {
  if (Boolean(parentPkgNodeId) !== Boolean(cachedNodeId)) return false
  if (parentPkgNodeId === cachedNodeId) return true
  if (!parentPkgNodeId) return false
  if (
    ctx.pathsByNodeId.has(cachedNodeId) &&
    ctx.pathsByNodeId.get(cachedNodeId) === ctx.pathsByNodeId.get(parentPkgNodeId)
  ) return true
  if (isLinkOutsideTheTree(ctx.dependenciesTree, parentPkgNodeId)) {
    return false
  }
  const parentPkgId = (ctx.dependenciesTree.get(parentPkgNodeId)!.resolvedPackage as Pkg).pkgIdWithPatchHash
  const cachedPkgId = (ctx.dependenciesTree.get(cachedNodeId)!.resolvedPackage as Pkg).pkgIdWithPatchHash
  if (parentPkgId !== cachedPkgId) {
    return false
  }
  return ctx.purePkgs.has(parentPkgId) || parentPackagesMatch(ctx, cachedNodeId, parentPkgNodeId)
}

function isLinkOutsideTheTree<Pkg> (dependenciesTree: DependenciesTree<Pkg>, nodeId: NodeId): boolean {
  return !dependenciesTree.has(nodeId) && typeof nodeId === 'string' && nodeId.startsWith('link:')
}

function parentPackagesMatch (ctx: ParentPackagesContext, cachedNodeId: NodeId, checkedNodeId: NodeId): boolean {
  const cachedParentPkgs = ctx.parentPkgsOfNode.get(cachedNodeId)
  if (!cachedParentPkgs) return false
  const checkedParentPkgs = ctx.parentPkgsOfNode.get(checkedNodeId)
  if (!checkedParentPkgs) return false
  if (Object.keys(cachedParentPkgs).length !== Object.keys(checkedParentPkgs).length) return false
  const maxDepth = Object.values(checkedParentPkgs)
    .reduce((maxDepth, { depth }) => Math.max(depth ?? 0, maxDepth), 0)
  const peerDepsAreNotShadowed = parentPkgsHaveSingleOccurrence(cachedParentPkgs) &&
    parentPkgsHaveSingleOccurrence(checkedParentPkgs)
  return (
    Object.entries(cachedParentPkgs).every(([name, cachedParentPkg]) =>
      parentPkgMatches(ctx, {
        cachedParentPkg,
        checkedParentPkg: checkedParentPkgs[name],
        maxDepth,
        peerDepsAreNotShadowed,
      })
    )
  )
}

function parentPkgMatches (
  ctx: ParentPackagesContext,
  opts: {
    cachedParentPkg: ParentPkgInfo
    checkedParentPkg: ParentPkgInfo | undefined
    maxDepth: number
    peerDepsAreNotShadowed: boolean
  }
): boolean {
  const { version, pkgIdWithPatchHash } = opts.cachedParentPkg
  const { checkedParentPkg } = opts
  if (checkedParentPkg == null) return false
  if (version && checkedParentPkg.version) {
    return version === checkedParentPkg.version
  }
  return pkgIdWithPatchHash != null &&
    (pkgIdWithPatchHash === checkedParentPkg.pkgIdWithPatchHash) &&
    (
      opts.peerDepsAreNotShadowed ||
      // Peer dependencies that appear last we can consider valid.
      // If they do depend on other peer dependencies then they must be those that we will check further.
      checkedParentPkg.depth === opts.maxDepth ||
      ctx.purePkgs.has(pkgIdWithPatchHash)
    )
}

function parentPkgsHaveSingleOccurrence (parentPkgs: Record<string, ParentPkgInfo>): boolean {
  return Object.values(parentPkgs).every(({ occurrence }) => occurrence === 0 || occurrence == null)
}
