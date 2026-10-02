import type { DepPath } from '@pnpm/types'
import { analyzeGraph, type Graph } from 'graph-cycles'
import pDefer from 'p-defer'
import { partition } from 'ramda'

import { addDepPathOrDeferCalculation } from './addNodeDepPath.js'
import { findHit } from './findPeersCacheHit.js'
import { pinLockedPeerProviders } from './lockedPeerProviders.js'
import type { NodeId } from './nextNodeId.js'
import { parentPkgsMatch, toPkgByName } from './parentRefs.js'
import { inheritedParentPkgBreaksPeerDiamond } from './peerDiamond.js'
import type {
  CalculateDepPath,
  FinishingResolutionPromise,
  MissingPeerInfo,
  MissingPeers,
  ParentPkgInfo,
  ParentPkgNode,
  ParentRefs,
  PartialResolvedPackage,
  PeersCacheItem,
  PeersResolution,
  ResolveNodePeersContext,
} from './peersResolutionTypes.js'
import type { ChildrenMap, DependenciesTree, DependenciesTreeNode } from './resolveDependencies.js'
import { getLocationFromParentNodeIds, resolvePeersFromParents } from './resolvePeersFromParents.js'

type NodePeersResolution = PeersResolution & { finishing?: FinishingResolutionPromise, calculateDepPath?: CalculateDepPath }

interface ChildrenPeersCollector {
  allResolvedPeers: Map<string, NodeId>
  allMissingPeers: MissingPeers
  calculateDepPaths: CalculateDepPath[]
  graph: Map<string, string[]>
  finishingList: FinishingResolutionPromise[]
}

export async function resolvePeersOfNode<Pkg extends PartialResolvedPackage> (
  currentAlias: string,
  nodeId: NodeId,
  parentParentPkgs: ParentRefs,
  ctx: ResolveNodePeersContext<Pkg>
): Promise<NodePeersResolution> {
  const node = ctx.dependenciesTree.get(nodeId)!
  if (node.depth === -1) return createEmptyPeersResolution()
  const resolvedPackage = node.resolvedPackage as Pkg
  if (
    ctx.purePkgs.has(resolvedPackage.pkgIdWithPatchHash) &&
    ctx.depGraph[resolvedPackage.pkgIdWithPatchHash as unknown as DepPath].depth <= node.depth &&
    Object.keys(resolvedPackage.peerDependencies).length === 0
  ) {
    ctx.pathsByNodeId.set(nodeId, resolvedPackage.pkgIdWithPatchHash as unknown as DepPath)
    ctx.pathsByNodeIdPromises.get(nodeId)!.resolve(resolvedPackage.pkgIdWithPatchHash as unknown as DepPath)
    return createEmptyPeersResolution()
  }
  if (typeof node.children === 'function') {
    node.children = node.children()
  }
  const parentNodeIds = [...ctx.parentNodeIds, nodeId]
  const children = node.children
  const parentPkgs = pinLockedPeerProviders(ctx, {
    node,
    resolvedPackage,
    parentPkgs: addChildrenToParentPkgs(ctx, { parentParentPkgs, children, parentNodeIds }),
    parentParentPkgs,
    parentNodeIds,
  })
  const hit = findHit(ctx, parentPkgs, resolvedPackage.pkgIdWithPatchHash)
  if (hit != null) {
    return reusePeersCacheHit(ctx, { hit, nodeId, node, parentNodeIds })
  }
  return resolveUncachedPeersOfNode(ctx, { currentAlias, nodeId, node, resolvedPackage, children, parentPkgs, parentNodeIds })
}

function createEmptyPeersResolution (): PeersResolution {
  return { resolvedPeers: new Map<string, NodeId>(), missingPeers: new Map<string, MissingPeerInfo>() }
}

function addChildrenToParentPkgs<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    parentParentPkgs: ParentRefs
    children: ChildrenMap
    parentNodeIds: NodeId[]
  }
): ParentRefs {
  const { children } = opts
  if (Object.keys(children).length === 0) return opts.parentParentPkgs
  const parentPkgs = { ...opts.parentParentPkgs }
  const newParentPkgs = toPkgByName(getPeerProvidingChildren(ctx, opts))
  for (const [newParentPkgName, newParentPkg] of Object.entries(newParentPkgs)) {
    const inheritedParentPkg = parentPkgs[newParentPkgName]
    if (!inheritedParentPkg) {
      parentPkgs[newParentPkgName] = newParentPkg
      continue
    }
    if (
      parentPkgsMatch(ctx.dependenciesTree, inheritedParentPkg, newParentPkg) &&
      !inheritedParentPkgBreaksPeerDiamond(ctx, parentPkgs, { inheritedParentPkg, ownChildParentPkg: newParentPkg, children })
    ) continue
    newParentPkg.occurrence = inheritedParentPkg.occurrence + 1
    parentPkgs[newParentPkgName] = newParentPkg
  }
  return parentPkgs
}

function getPeerProvidingChildren<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    children: ChildrenMap
    parentNodeIds: NodeId[]
  }
): Array<ParentPkgNode<Pkg>> {
  const parentPkgNodes: Array<ParentPkgNode<Pkg>> = []
  for (const [alias, nodeId] of Object.entries(opts.children)) {
    const childNode = ctx.dependenciesTree.get(nodeId)!
    if (ctx.allPeerDepNames.has(alias) || (alias !== childNode.resolvedPackage.name && ctx.allPeerDepNames.has(childNode.resolvedPackage.name))) {
      parentPkgNodes.push({
        alias,
        node: childNode,
        nodeId,
        parentNodeIds: opts.parentNodeIds,
      })
    }
  }
  return parentPkgNodes
}

function reusePeersCacheHit<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    hit: PeersCacheItem
    nodeId: NodeId
    node: DependenciesTreeNode<Pkg>
    parentNodeIds: NodeId[]
  }
): NodePeersResolution {
  const { hit } = opts
  for (const [peerName, { range: wantedRange, optional }] of hit.missingPeers.entries()) {
    if (ctx.peerDependencyIssues.missing[peerName] == null) {
      ctx.peerDependencyIssues.missing[peerName] = []
    }
    const { parents } = getLocationFromParentNodeIds({
      dependenciesTree: ctx.dependenciesTree,
      parentNodeIds: opts.parentNodeIds,
    })
    ctx.peerDependencyIssues.missing[peerName].push({
      optional,
      parents,
      wantedRange,
    })
  }
  ctx.peersCacheOwnerByNodeId.set(opts.nodeId, hit.ownerNodeId)
  return {
    missingPeers: hit.missingPeers,
    finishing: finishWithCachedDepPath(ctx, opts),
    resolvedPeers: hit.resolvedPeers,
  }
}

async function finishWithCachedDepPath<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  { hit, nodeId, node }: { hit: PeersCacheItem, nodeId: NodeId, node: DependenciesTreeNode<Pkg> }
): Promise<void> {
  const depPath = await hit.depPath.promise
  ctx.pathsByNodeId.set(nodeId, depPath)
  ctx.depGraph[depPath].depth = Math.min(ctx.depGraph[depPath].depth, node.depth)
  ctx.pathsByNodeIdPromises.get(nodeId)!.resolve(depPath)
}

async function resolveUncachedPeersOfNode<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    currentAlias: string
    nodeId: NodeId
    node: DependenciesTreeNode<Pkg>
    resolvedPackage: Pkg
    children: ChildrenMap
    parentPkgs: ParentRefs
    parentNodeIds: NodeId[]
  }
): Promise<NodePeersResolution> {
  const { node, nodeId, resolvedPackage, parentNodeIds } = opts
  const childrenResolution = await resolvePeersOfChildren(opts.children, opts.parentPkgs, {
    ...ctx,
    parentNodeIds,
    parentDepPathsChain: ctx.parentDepPathsChain.includes(resolvedPackage.pkgIdWithPatchHash) ? ctx.parentDepPathsChain : [...ctx.parentDepPathsChain, resolvedPackage.pkgIdWithPatchHash],
  })

  const ownPeers = Object.keys(resolvedPackage.peerDependencies).length === 0
    ? createEmptyPeersResolution()
    : resolvePeersFromParents({
      currentDepth: node.depth,
      dependenciesTree: ctx.dependenciesTree,
      lockfileDir: ctx.lockfileDir,
      nodeId,
      parentPkgs: opts.parentPkgs,
      peerDependencyIssues: ctx.peerDependencyIssues,
      resolvedPackage,
      rootDir: ctx.rootDir,
      parentNodeIds,
    })

  const allResolvedPeers = childrenResolution.resolvedPeers
  for (const [peerName, peerNodeId] of ownPeers.resolvedPeers) {
    allResolvedPeers.set(peerName, peerNodeId)
  }
  allResolvedPeers.delete(node.resolvedPackage.name)
  const allMissingPeers = new Map<string, MissingPeerInfo>([...childrenResolution.missingPeers, ...ownPeers.missingPeers])

  const isPure = allResolvedPeers.size === 0 && allMissingPeers.size === 0
  return {
    resolvedPeers: allResolvedPeers,
    missingPeers: allMissingPeers,
    calculateDepPath: addDepPathOrDeferCalculation(ctx, {
      ...opts,
      cache: cachePeersResolution(ctx, { resolvedPackage, nodeId, isPure, allResolvedPeers, allMissingPeers }),
      isPure,
      allResolvedPeers,
      ownResolvedPeers: ownPeers.resolvedPeers,
      missingPeersOfChildren: childrenResolution.missingPeers,
    }),
    finishing: childrenResolution.finishing,
  }
}

function cachePeersResolution<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    resolvedPackage: Pkg
    nodeId: NodeId
    isPure: boolean
    allResolvedPeers: Map<string, NodeId>
    allMissingPeers: MissingPeers
  }
): PeersCacheItem | undefined {
  const { pkgIdWithPatchHash } = opts.resolvedPackage
  // A cycle re-entry resolves against truncated children (the cycle is broken by
  // dropping the repeated package's subtree), so its empty/partial peer sets are
  // not authoritative for the package as a whole. Recording that verdict — as
  // pure or in peersCache — lets it short-circuit other occurrences that *can*
  // see the full subtree, dropping their transitivePeerDependencies depending on
  // traversal order and churning the lockfile (https://github.com/pnpm/pnpm/issues/5108).
  const resolvedThroughCycle = ctx.parentDepPathsChain.includes(pkgIdWithPatchHash)
  if (resolvedThroughCycle) {
    // Leave both caches untouched so later occurrences re-resolve (or hit the
    // authoritative entry of the same package) instead of reusing this partial one.
    return undefined
  }
  if (opts.isPure) {
    ctx.purePkgs.add(pkgIdWithPatchHash)
    return undefined
  }
  const cache: PeersCacheItem = {
    missingPeers: opts.allMissingPeers,
    depPath: pDefer(),
    resolvedPeers: opts.allResolvedPeers,
    ownerNodeId: opts.nodeId,
  }
  if (ctx.peersCache.has(pkgIdWithPatchHash)) {
    ctx.peersCache.get(pkgIdWithPatchHash)!.push(cache)
  } else {
    ctx.peersCache.set(pkgIdWithPatchHash, [cache])
  }
  return cache
}

export async function resolvePeersOfChildren<Pkg extends PartialResolvedPackage> (
  children: {
    [alias: string]: NodeId
  },
  parentPkgs: ParentRefs,
  ctx: ResolveNodePeersContext<Pkg>
): Promise<PeersResolution & { finishing: Promise<void> }> {
  const nodeIds = getChildNodeIdsInResolutionOrder(children, parentPkgs)
  const aliasByNodeId = Object.fromEntries(Object.entries(children).map(([alias, nodeId]) => [nodeId, alias]))

  for (const nodeId of nodeIds) {
    if (!ctx.pathsByNodeIdPromises.has(nodeId)) {
      ctx.pathsByNodeIdPromises.set(nodeId, pDefer())
    }
  }

  // Resolving non-repeated nodes before repeated nodes proved to be slightly faster.
  const parentDepPaths = getParentPkgInfos(ctx, parentPkgs)
  for (const childNodeId of nodeIds) {
    ctx.parentPkgsOfNode.set(childNodeId, parentDepPaths)
  }
  const collected = await resolvePeersOfEachChild(ctx, { nodeIds, aliasByNodeId, parentPkgs })
  if (collected.calculateDepPaths.length) {
    const { cycles } = analyzeGraph(Array.from(collected.graph.entries()) as unknown as Graph) as unknown as { cycles: string[][] }
    collected.finishingList.push(...collected.calculateDepPaths.map((calculateDepPath) => calculateDepPath(cycles)))
  }
  const finishing = Promise.all(collected.finishingList).then(() => {})

  const unknownResolvedPeersOfChildren = new Map<string, NodeId>()
  for (const [alias, peerNodeId] of collected.allResolvedPeers) {
    if (!children[alias]) {
      unknownResolvedPeersOfChildren.set(alias, peerNodeId)
    }
  }

  return { resolvedPeers: unknownResolvedPeersOfChildren, missingPeers: collected.allMissingPeers, finishing }
}

// Partition children based on whether they're repeated in parentPkgs.
// This impacts the efficiency of graph traversal and prevents potential out-of-memory errors.
// We check repeated first as the peers resolution of those probably are cached already.
function getChildNodeIdsInResolutionOrder (children: ChildrenMap, parentPkgs: ParentRefs): NodeId[] {
  const [repeated, notRepeated] = partition(([alias]) => parentPkgs[alias] != null, Object.entries(children))
  return Array.from(new Set([...repeated, ...notRepeated].map(([, nodeId]) => nodeId)))
}

function getParentPkgInfos<Pkg extends PartialResolvedPackage> (
  ctx: { allPeerDepNames: Set<string>, dependenciesTree: DependenciesTree<Pkg> },
  parentPkgs: ParentRefs
): Record<string, ParentPkgInfo> {
  const parentDepPaths: Record<string, ParentPkgInfo> = {}
  for (const [name, parentPkg] of Object.entries(parentPkgs)) {
    if (!ctx.allPeerDepNames.has(name)) continue
    if (parentPkg.nodeId && (typeof parentPkg.nodeId === 'number' || !parentPkg.nodeId.startsWith('link:'))) {
      parentDepPaths[name] = {
        pkgIdWithPatchHash: (ctx.dependenciesTree.get(parentPkg.nodeId)!.resolvedPackage as Pkg).pkgIdWithPatchHash,
        depth: parentPkg.depth,
        occurrence: parentPkg.occurrence,
      }
    } else {
      parentDepPaths[name] = { version: parentPkg.version }
    }
  }
  return parentDepPaths
}

async function resolvePeersOfEachChild<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    nodeIds: NodeId[]
    aliasByNodeId: Record<string, string>
    parentPkgs: ParentRefs
  }
): Promise<ChildrenPeersCollector> {
  const collected: ChildrenPeersCollector = {
    allResolvedPeers: new Map<string, NodeId>(),
    allMissingPeers: new Map<string, MissingPeerInfo>(),
    calculateDepPaths: [],
    graph: new Map(),
    finishingList: [],
  }
  for (const childNodeId of opts.nodeIds) {
    const currentAlias = opts.aliasByNodeId[childNodeId]
    const nodeResolution = await resolvePeersOfNode(currentAlias, childNodeId, opts.parentPkgs, ctx) // eslint-disable-line no-await-in-loop -- repeated children go first so they hit the peers cache filled by their earlier siblings
    collectChildPeersResolution(ctx.dependenciesTree, collected, { currentAlias, childNodeId, nodeResolution })
  }
  return collected
}

function collectChildPeersResolution<Pkg> (
  dependenciesTree: DependenciesTree<Pkg>,
  collected: ChildrenPeersCollector,
  { currentAlias, childNodeId, nodeResolution }: {
    currentAlias: string
    childNodeId: NodeId
    nodeResolution: NodePeersResolution
  }
): void {
  if (nodeResolution.finishing) {
    collected.finishingList.push(nodeResolution.finishing)
  }
  if (nodeResolution.calculateDepPath) {
    collected.calculateDepPaths.push(nodeResolution.calculateDepPath)
  }
  const edges: string[] = []
  for (const [peerName, peerNodeId] of nodeResolution.resolvedPeers) {
    collected.allResolvedPeers.set(peerName, peerNodeId)
    edges.push(peerName)
  }
  addEdgesToGraph(collected.graph, currentAlias, edges)
  const node = dependenciesTree.get(childNodeId)!
  // We resolve peer dependencies via both the alias and the real name of the package.
  // That's why we need to detect circular graphs via both the alias and the real name.
  if (currentAlias !== node.resolvedPackage.name) {
    addEdgesToGraph(collected.graph, node.resolvedPackage.name, edges)
  }
  for (const [missingPeer, range] of nodeResolution.missingPeers.entries()) {
    collected.allMissingPeers.set(missingPeer, range)
  }
}

function addEdgesToGraph (graph: Map<string, string[]>, pkgName: string, edges: string[]): void {
  const existingEdges = graph.get(pkgName)
  if (existingEdges == null) {
    graph.set(pkgName, edges)
  } else {
    existingEdges.push(...edges)
  }
}
