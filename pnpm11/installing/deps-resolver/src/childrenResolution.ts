import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import pDefer from 'p-defer'
import { pShare } from 'promise-share'

import { filterMissingPeers } from './missingPeers.js'
import { nextNodeId, type NodeId } from './nextNodeId.js'
import { parentIdsContainSequence } from './parentIdsContainSequence.js'
import type {
  ChildrenByParentId,
  ChildrenResolution,
  ChildrenResolutionOwner,
  DependenciesTree,
  MissingPeers,
  MissingPeersOfChildren,
  ParentPkgAliases,
  PeersResolutionResult,
  PkgAddress,
  ResolutionContext,
  ResolvedPackage,
  ResolvedPkgsById,
} from './resolutionTypes.js'

export interface ClaimedChildrenResolution {
  id: number
  isOwner: boolean
  missingPeersOfChildren?: MissingPeersOfChildren
}

export function claimChildrenResolution (
  ctx: ResolutionContext,
  opts: {
    currentDepth: number
    parentIds: PkgResolutionId[]
    pkgId: PkgResolutionId
  }
): ClaimedChildrenResolution {
  const owner = {
    depth: opts.currentDepth,
    importerOrder: ctx.importerResolutionOrder[opts.parentIds[0]] ?? Number.MAX_SAFE_INTEGER,
    parentPath: opts.parentIds,
  }
  const existing = ctx.childrenResolutionByPkgId[opts.pkgId]
  if (existing == null || compareChildrenResolutionOwners(owner, existing.owner) < 0) {
    return takeOverChildrenResolution(ctx, { owner, pkgId: opts.pkgId, previous: existing })
  }
  return {
    id: existing.id,
    isOwner: false,
    missingPeersOfChildren: canReuseMissingPeersOfChildren(ctx, opts, existing) ? existing.missingPeersOfChildren : undefined,
  }
}

function takeOverChildrenResolution (
  ctx: ResolutionContext,
  { owner, pkgId, previous }: {
    owner: ChildrenResolutionOwner
    pkgId: PkgResolutionId
    previous?: ChildrenResolution
  }
): ClaimedChildrenResolution {
  const missingPeersOfChildren = ctx.hoistPeers && !owner.parentPath.includes(pkgId)
    ? createMissingPeersOfChildren()
    : undefined
  const resolution = {
    id: ++ctx.childrenResolutionId,
    owner,
    missingPeersOfChildren,
  }
  ctx.childrenResolutionByPkgId[pkgId] = resolution
  if (missingPeersOfChildren) {
    ctx.missingPeersOfChildrenByPkgId[pkgId] = {
      depth: owner.depth,
      missingPeersOfChildren,
    }
  }
  if (previous?.missingPeersOfChildren) {
    forwardMissingPeersOfChildren(previous.missingPeersOfChildren, missingPeersOfChildren)
  }
  return {
    id: resolution.id,
    isOwner: true,
    missingPeersOfChildren,
  }
}

function forwardMissingPeersOfChildren (
  previous: MissingPeersOfChildren,
  current: MissingPeersOfChildren | undefined
): void {
  if (current) {
    current.get().then((missingPeers) => {
      previous.resolved = true
      previous.resolve(missingPeers)
    }, previous.reject)
    return
  }
  previous.resolved = true
  previous.resolve({})
}

function canReuseMissingPeersOfChildren (
  ctx: ResolutionContext,
  opts: { currentDepth: number, parentIds: PkgResolutionId[], pkgId: PkgResolutionId },
  existing: ChildrenResolution
): boolean {
  // Gating reuse on the owner's depth keeps a transitive optional peer's
  // presence in the resolved suffix a function of graph structure, not of
  // which occurrence happened to finish resolving first. A strictly shallower
  // owner is excluded because its promise may still be unsettled here, and
  // awaiting it can deadlock under auto-install-peers
  // (https://github.com/pnpm/pnpm/issues/5454).
  return Boolean(
    ctx.hoistPeers &&
    !opts.parentIds.includes(opts.pkgId) &&
    existing.missingPeersOfChildren &&
    existing.owner.depth >= opts.currentDepth
  )
}

function compareChildrenResolutionOwners (owner1: ChildrenResolutionOwner, owner2: ChildrenResolutionOwner): number {
  if (owner1.depth !== owner2.depth) return owner1.depth - owner2.depth
  if (owner1.importerOrder !== owner2.importerOrder) return owner1.importerOrder - owner2.importerOrder
  const pathLength = Math.min(owner1.parentPath.length, owner2.parentPath.length)
  for (let index = 0; index < pathLength; index++) {
    const result = lexCompare(owner1.parentPath[index], owner2.parentPath[index])
    if (result !== 0) return result
  }
  return owner1.parentPath.length - owner2.parentPath.length
}

function createMissingPeersOfChildren (): MissingPeersOfChildren {
  const missingPeers = pDefer<MissingPeers>()
  return {
    resolve: missingPeers.resolve,
    reject: missingPeers.reject,
    get: pShare(missingPeers.promise),
  }
}

export function isCurrentChildrenResolution (
  ctx: ResolutionContext,
  pkgId: PkgResolutionId,
  childrenResolutionId?: number
): boolean {
  return childrenResolutionId != null && ctx.childrenResolutionByPkgId[pkgId]?.id === childrenResolutionId
}

export async function resolveMissingPeersFromCurrentChildrenResolution (
  ctx: ResolutionContext,
  pkgId: PkgResolutionId,
  parentPkgAliases: ParentPkgAliases
): Promise<PeersResolutionResult> {
  const missingPeersOfChildren = ctx.childrenResolutionByPkgId[pkgId]?.missingPeersOfChildren
  if (missingPeersOfChildren == null) {
    return {
      missingPeers: {},
      resolvedPeers: {},
    }
  }
  const missingPeers = await missingPeersOfChildren.get()
  return filterMissingPeers({ missingPeers, resolvedPeers: {} }, parentPkgAliases)
}

export function setDependencyTreeNodeWithCurrentChildren (
  ctx: ResolutionContext,
  {
    parentDepth,
    parentIds,
    parentPkg,
  }: {
    parentDepth: number
    parentIds: PkgResolutionId[]
    parentPkg: PkgAddress
  }
): void {
  ctx.dependenciesTree.set(parentPkg.nodeId, {
    children: () => buildTree(ctx, parentPkg.pkgId, parentIds, ctx.childrenByParentId[parentPkg.pkgId] ?? [], parentDepth + 1, parentPkg.installable),
    depth: parentDepth,
    installable: parentPkg.installable,
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
}

export function updateChildrenResolutionNodes (
  ctx: ResolutionContext,
  pkgId: PkgResolutionId,
  currentNodeId: NodeId
): void {
  for (const [nodeId, nodeContext] of ctx.nodeResolutionContextByNodeId) {
    if (nodeId === currentNodeId || nodeContext.pkgId !== pkgId) continue
    const node = ctx.dependenciesTree.get(nodeId)
    if (node == null || node.depth === -1) continue
    node.children = () => buildTree(ctx, pkgId, nodeContext.parentIds, ctx.childrenByParentId[pkgId] ?? [], nodeContext.depth + 1, nodeContext.installable)
  }
}

export function buildTree (
  ctx: {
    childrenByParentId: ChildrenByParentId
    dependenciesTree: DependenciesTree<ResolvedPackage>
    resolvedPkgsById: ResolvedPkgsById
    skipped: Set<PkgResolutionId>
  },
  parentId: PkgResolutionId,
  parentIds: PkgResolutionId[],
  children: Array<{ alias: string, id: PkgResolutionId }>,
  depth: number,
  installable: boolean
): Record<string, NodeId> {
  const childrenNodeIds: Record<string, NodeId> = {}
  for (const child of children) {
    if (child.id.startsWith('link:')) {
      childrenNodeIds[child.alias] = child.id as unknown as NodeId
      continue
    }
    if (parentIdsContainSequence(parentIds, parentId, child.id) || parentId === child.id) {
      continue
    }
    if (ctx.resolvedPkgsById[child.id].isLeaf) {
      childrenNodeIds[child.alias] = child.id as unknown as NodeId
      continue
    }
    const childNodeId = nextNodeId()
    childrenNodeIds[child.alias] = childNodeId
    installable = installable || !ctx.skipped.has(child.id)
    ctx.dependenciesTree.set(childNodeId, {
      children: () => buildTree(ctx,
        child.id,
        [...parentIds, child.id],
        ctx.childrenByParentId[child.id],
        depth + 1,
        installable
      ),
      depth,
      installable,
      resolvedPackage: ctx.resolvedPkgsById[child.id],
    })
  }
  return childrenNodeIds
}
