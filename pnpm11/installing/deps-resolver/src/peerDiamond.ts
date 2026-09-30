import type { PkgIdWithPatchHash } from '@pnpm/types'

import type { NodeId } from './nextNodeId.js'
import type {
  ParentPkgInfo,
  ParentPkgsOfNode,
  ParentRef,
  ParentRefs,
  PartialResolvedPackage,
} from './peersResolutionTypes.js'
import type { ChildrenMap, DependenciesTree, DependenciesTreeNode } from './resolveDependencies.js'

interface PeerDiamondContext<Pkg extends PartialResolvedPackage> {
  dependenciesTree: DependenciesTree<Pkg>
  parentPkgsOfNode: ParentPkgsOfNode
  allPeerDepNames: Set<string>
}

// A package that is both inherited from an ancestor and present among the
// current node's own children is normally not duplicated: the inherited
// instance is reused (see https://github.com/pnpm/pnpm/issues/8370). That reuse
// is unsafe when a sibling peer-depends both this package and one of this
// package's own peer dependencies. The sibling must see a single, consistent
// instance of that shared peer, but the inherited instance resolved it in a
// different context. In that case the node's own child has to be used instead.
// See https://github.com/pnpm/pnpm/issues/12079
export function inheritedParentPkgBreaksPeerDiamond<Pkg extends PartialResolvedPackage> (
  ctx: PeerDiamondContext<Pkg>,
  parentPkgs: ParentRefs,
  opts: {
    inheritedParentPkg: ParentRef
    ownChildParentPkg: ParentRef
    children: ChildrenMap
  }
): boolean {
  const { inheritedParentPkg, ownChildParentPkg } = opts
  if (inheritedParentPkg.nodeId == null || ownChildParentPkg.nodeId == null) return false
  if (inheritedParentPkg.nodeId === ownChildParentPkg.nodeId) return false
  const inheritedContext = ctx.parentPkgsOfNode.get(inheritedParentPkg.nodeId)
  if (inheritedContext == null) return false
  const parentPkg = ctx.dependenciesTree.get(ownChildParentPkg.nodeId)?.resolvedPackage as Pkg | undefined
  if (parentPkg == null) return false

  const conflictingPeers = getConflictingPeers(ctx, { parentPkg, parentPkgs, inheritedContext })
  if (conflictingPeers.size === 0) return false
  return descendantClosesPeerDiamond(ctx, { children: opts.children, parentPkg, conflictingPeers })
}

function getConflictingPeers<Pkg extends PartialResolvedPackage> (
  ctx: PeerDiamondContext<Pkg>,
  opts: {
    parentPkg: Pkg
    parentPkgs: ParentRefs
    inheritedContext: Record<string, ParentPkgInfo>
  }
): Set<string> {
  const conflictingPeers = new Set<string>()
  for (const peerName of Object.keys(opts.parentPkg.peerDependencies ?? {})) {
    if (!ctx.allPeerDepNames.has(peerName)) continue
    const inheritedPeer = opts.inheritedContext[peerName]
    const currentPeer = opts.parentPkgs[peerName]
    if (inheritedPeer == null || currentPeer == null) continue
    if (parentPeerDiffers(ctx.dependenciesTree, currentPeer, inheritedPeer)) {
      conflictingPeers.add(peerName)
    }
  }
  return conflictingPeers
}

// The consumer that closes the diamond may be any descendant that inherits
// this node's provider, not only a direct child. The children of a node
// depend only on its package, so each package's children are expanded once.
// A package is marked only when it inherits the provider: cycle pruning can
// drop its copy of the provider from one occurrence but not another.
// See https://github.com/pnpm/pnpm/issues/12098
function descendantClosesPeerDiamond<Pkg extends PartialResolvedPackage> (
  ctx: PeerDiamondContext<Pkg>,
  opts: {
    children: ChildrenMap
    parentPkg: Pkg
    conflictingPeers: Set<string>
  }
): boolean {
  const { parentPkg } = opts
  const visited = new Set<PkgIdWithPatchHash>()
  const pending = Object.values(opts.children)
  let childNodeId: NodeId | undefined
  while ((childNodeId = pending.pop()) != null) {
    const childNode = ctx.dependenciesTree.get(childNodeId)
    if (childNode == null) continue
    const childPkg = childNode.resolvedPackage as Pkg
    if (visited.has(childPkg.pkgIdWithPatchHash)) continue
    if (peerDependsOnPkgAndAnyPeer(childPkg.peerDependencies, parentPkg.name, opts.conflictingPeers)) return true
    const grandchildren = expandChildren(childNode)
    // A descendant that has its own copy of the package provides it to its
    // subtree, so the inherited one doesn't reach any deeper.
    if (childrenProvidePkg(ctx.dependenciesTree, grandchildren, parentPkg.name)) continue
    visited.add(childPkg.pkgIdWithPatchHash)
    pending.push(...Object.values(grandchildren))
  }
  return false
}

function expandChildren<Pkg> (node: DependenciesTreeNode<Pkg>): ChildrenMap {
  if (typeof node.children === 'function') {
    node.children = node.children()
  }
  return node.children
}

function peerDependsOnPkgAndAnyPeer (
  peerDependencies: PartialResolvedPackage['peerDependencies'] | undefined,
  pkgName: string,
  peerNames: Set<string>
): boolean {
  if (peerDependencies?.[pkgName] == null) return false
  for (const peerName of peerNames) {
    if (peerDependencies[peerName] != null) return true
  }
  return false
}

// An aliased child provides peers under its real package name too, as it does
// in toPkgByName.
function childrenProvidePkg<Pkg extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<Pkg>,
  children: ChildrenMap,
  pkgName: string
): boolean {
  return Object.entries(children).some(([alias, nodeId]) =>
    alias === pkgName || dependenciesTree.get(nodeId)?.resolvedPackage.name === pkgName
  )
}

function parentPeerDiffers<Pkg extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<Pkg>,
  currentPeer: ParentRef,
  inheritedPeer: ParentPkgInfo
): boolean {
  if (inheritedPeer.pkgIdWithPatchHash != null) {
    if (currentPeer.nodeId == null || (typeof currentPeer.nodeId === 'string' && currentPeer.nodeId.startsWith('link:'))) {
      return true
    }
    return (dependenciesTree.get(currentPeer.nodeId)?.resolvedPackage as Pkg | undefined)?.pkgIdWithPatchHash !== inheritedPeer.pkgIdWithPatchHash
  }
  return currentPeer.version !== inheritedPeer.version
}
