import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import { createPeerDepGraphHash, depPathToFilename, type PeerId } from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import { linkPathToPeerVersion } from './linkPathToPeerVersion.js'
import type { NodeId } from './nextNodeId.js'
import type {
  CalculateDepPath,
  MissingPeers,
  PartialResolvedPackage,
  PeersCacheItem,
  PendingPeer,
  ResolveNodePeersContext,
  ResolvePeersContext,
} from './peersResolutionTypes.js'
import type { ChildrenMap, DependenciesTree, DependenciesTreeNode } from './resolveDependencies.js'

/**
 * A node whose peers are resolved and whose dep path is still to be added to
 * the graph.
 */
export interface NodeWithResolvedPeers<Pkg extends PartialResolvedPackage> {
  currentAlias: string
  nodeId: NodeId
  node: DependenciesTreeNode<Pkg>
  resolvedPackage: Pkg
  children: ChildrenMap
  cache?: PeersCacheItem
  isPure: boolean
  allResolvedPeers: Map<string, NodeId>
  ownResolvedPeers: Map<string, NodeId>
  missingPeersOfChildren: MissingPeers
}

/**
 * Adds the node's dep path to the graph right away when every peer's dep path
 * is known, and otherwise returns the calculation to run once the peer cycles
 * among its siblings are known.
 */
export function addDepPathOrDeferCalculation<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  resolvedNode: NodeWithResolvedPeers<Pkg>
): CalculateDepPath | undefined {
  const { pkgIdWithPatchHash } = resolvedNode.resolvedPackage
  if (resolvedNode.allResolvedPeers.size === 0) {
    addDepPathToGraph(ctx, resolvedNode, pkgIdWithPatchHash as unknown as DepPath)
    return undefined
  }
  const peerIds: PeerId[] = []
  const pendingPeers: PendingPeer[] = []
  for (const [alias, peerNodeId] of resolvedNode.allResolvedPeers.entries()) {
    const peerId = peerNodeIdToPeerId(alias, peerNodeId, ctx)
    if (peerId != null) {
      peerIds.push(peerId)
    } else {
      pendingPeers.push({ alias, nodeId: peerNodeId })
    }
  }
  if (pendingPeers.length === 0) {
    const peerDepGraphHash = createPeerDepGraphHash(peerIds, ctx.peersSuffixMaxLength)
    addDepPathToGraph(ctx, resolvedNode, `${pkgIdWithPatchHash}${peerDepGraphHash}` as DepPath)
    return undefined
  }
  return async (cycles) => calculateDepPath(ctx, { resolvedNode, peerIds, pendingPeerNodes: pendingPeers }, cycles)
}

async function calculateDepPath<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    resolvedNode: NodeWithResolvedPeers<Pkg>
    peerIds: PeerId[]
    pendingPeerNodes: PendingPeer[]
  },
  cycles: string[][]
): Promise<void> {
  const { resolvedNode, pendingPeerNodes } = opts
  const cyclicPeerAliases = getCyclicPeerAliases(cycles, resolvedNode.currentAlias, pendingPeerNodes)
  const peerDepGraphHash = createPeerDepGraphHash([
    ...opts.peerIds,
    ...await Promise.all(pendingPeerNodes
      .map(async (pendingPeer) => getPendingPeerId(ctx, { nodeId: resolvedNode.nodeId, cyclicPeerAliases, pendingPeer }))
    ),
  ], ctx.peersSuffixMaxLength)
  addDepPathToGraph(ctx, resolvedNode, `${resolvedNode.resolvedPackage.pkgIdWithPatchHash}${peerDepGraphHash}` as DepPath)
}

function getCyclicPeerAliases (
  cycles: string[][],
  currentAlias: string,
  pendingPeerNodes: PendingPeer[]
): Set<string> {
  const cyclicPeerAliases = new Set<string>()
  const pendingPeerAliases = new Set(pendingPeerNodes.map(({ alias }) => alias))
  for (const cycle of cycles) {
    // A cycle has to be short-circuited at this level whenever any of
    // its members is involved in the current resolution — either as
    // currentAlias or among the pending peers we are about to await.
    // When a cycle member hits the `findHit` cache instead of running
    // its own calculateDepPath, only the awaiting siblings at this
    // level can release the cached promise. See pnpm/pnpm#11999.
    if (cycle.includes(currentAlias) || cycle.some((alias) => pendingPeerAliases.has(alias))) {
      for (const peerAlias of cycle) {
        cyclicPeerAliases.add(peerAlias)
      }
    }
  }
  return cyclicPeerAliases
}

async function getPendingPeerId<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  opts: {
    nodeId: NodeId
    cyclicPeerAliases: Set<string>
    pendingPeer: PendingPeer
  }
): Promise<PeerId> {
  const { nodeId, pendingPeer } = opts
  if (opts.cyclicPeerAliases.has(pendingPeer.alias)) {
    const resolvedPeer = ctx.dependenciesTree.get(pendingPeer.nodeId)?.resolvedPackage as Pkg
    const { name, version } = peerIdOfResolvedPackage(resolvedPeer)
    const id = `${name}@${version}`
    ctx.cycleBrokenNodeIds.add(pendingPeer.nodeId)
    ctx.pathsByNodeIdPromises.get(pendingPeer.nodeId)?.resolve(id as DepPath)
    return id
  }
  if (ctx.dedupePeers) {
    const peerNode = ctx.dependenciesTree.get(pendingPeer.nodeId)
    if (peerNode) {
      return peerIdOfResolvedPackage(peerNode.resolvedPackage)
    }
  }
  let awaitedPeerNodeIds = ctx.awaitedPeerNodeIdsByNodeId.get(nodeId)
  if (awaitedPeerNodeIds == null) {
    awaitedPeerNodeIds = new Set()
    ctx.awaitedPeerNodeIdsByNodeId.set(nodeId, awaitedPeerNodeIds)
  }
  awaitedPeerNodeIds.add(pendingPeer.nodeId)
  return ctx.pathsByNodeIdPromises.get(pendingPeer.nodeId)!.promise
}

function addDepPathToGraph<Pkg extends PartialResolvedPackage> (
  ctx: ResolveNodePeersContext<Pkg>,
  resolvedNode: NodeWithResolvedPeers<Pkg>,
  depPath: DepPath
): void {
  const { nodeId, node, resolvedPackage } = resolvedNode
  resolvedNode.cache?.depPath.resolve(depPath)
  ctx.pathsByNodeId.set(nodeId, depPath)
  ctx.pathsByNodeIdPromises.get(nodeId)!.resolve(depPath)
  if (ctx.depPathsByPkgId != null) {
    recordDepPathOfPkg(ctx.depPathsByPkgId, resolvedPackage.pkgIdWithPatchHash, depPath)
  }
  const peerDependencies = { ...resolvedPackage.peerDependencies }
  if (ctx.depGraph[depPath] && ctx.depGraph[depPath].depth <= node.depth) return
  const modules = path.join(ctx.virtualStoreDir, depPathToFilename(depPath, ctx.virtualStoreDirMaxLength), 'node_modules')
  const dir = safeJoinModulesDir(modules, resolvedPackage.name)

  const transitivePeerDependencies = new Set<string>()
  for (const unknownPeer of [...resolvedNode.allResolvedPeers.keys(), ...resolvedNode.missingPeersOfChildren.keys()]) {
    if (!peerDependencies[unknownPeer]) {
      transitivePeerDependencies.add(unknownPeer)
    }
  }
  ctx.depGraph[depPath] = {
    ...(node.resolvedPackage as Pkg),
    childrenNodeIds: Object.assign(
      getPreviouslyResolvedChildren(ctx, (node.resolvedPackage as Pkg).pkgIdWithPatchHash),
      resolvedNode.children,
      Object.fromEntries(resolvedNode.ownResolvedPeers.entries())
    ),
    depPath,
    depth: node.depth,
    dir,
    installable: node.installable,
    isPure: resolvedNode.isPure,
    modules,
    peerDependencies,
    transitivePeerDependencies,
    resolvedPeerNames: new Set(resolvedNode.allResolvedPeers.keys()),
  }
}

function recordDepPathOfPkg (
  depPathsByPkgId: Map<PkgIdWithPatchHash, Set<DepPath>>,
  pkgIdWithPatchHash: PkgIdWithPatchHash,
  depPath: DepPath
): void {
  if (!depPathsByPkgId.has(pkgIdWithPatchHash)) {
    depPathsByPkgId.set(pkgIdWithPatchHash, new Set([depPath]))
  } else {
    depPathsByPkgId.get(pkgIdWithPatchHash)!.add(depPath)
  }
}

// When a package has itself in the subdependencies, so there's a cycle,
// pnpm will break the cycle, when it first repeats itself.
// However, when the cycle is broken up, the last repeated package is removed
// from the dependencies of the parent package.
// So we need to merge all the children of all the parent packages with same ID as the resolved package.
// This way we get all the children that were removed, when ending cycles.
function getPreviouslyResolvedChildren<Pkg extends PartialResolvedPackage> (
  {
    parentNodeIds,
    parentDepPathsChain,
    dependenciesTree,
  }: {
    parentNodeIds: NodeId[]
    parentDepPathsChain: PkgIdWithPatchHash[]
    dependenciesTree: DependenciesTree<Pkg>
  },
  currentDepPath: PkgIdWithPatchHash
): ChildrenMap {
  const allChildren: ChildrenMap = {}

  if (!currentDepPath || !parentDepPathsChain.includes(currentDepPath)) return allChildren

  for (let index = parentNodeIds.length - 1; index >= 0; index--) {
    const parentNode = dependenciesTree.get(parentNodeIds[index])!
    if ((parentNode.resolvedPackage as Pkg).pkgIdWithPatchHash === currentDepPath) {
      if (typeof parentNode.children === 'function') {
        parentNode.children = parentNode.children()
      }
      Object.assign(
        allChildren,
        parentNode.children
      )
    }
  }
  return allChildren
}

/**
 * The `name@version` identity a peer contributes to a dep path's peer suffix.
 *
 * A package resolved from a named registry keeps its `<registryName>:` in the
 * version slot. Dropping it would let the same name and version from two
 * registries produce one suffix, so two variants of the dependent — each
 * bound to a different peer artifact — would collapse onto a single depPath.
 */
function peerIdOfResolvedPackage (
  resolvedPackage: { name: string, version: string, id?: PkgResolutionId }
): { name: string, version: string } {
  // Importer and link nodes carry no resolution id and can never be
  // registry-qualified.
  const registryName = resolvedPackage.id == null
    ? undefined
    : dp.parse(resolvedPackage.id).registryName
  return {
    name: resolvedPackage.name,
    version: registryName == null
      ? resolvedPackage.version
      : `${registryName}:${resolvedPackage.version}`,
  }
}

function peerNodeIdToPeerId<Pkg extends PartialResolvedPackage> (
  alias: string,
  peerNodeId: NodeId,
  ctx: ResolvePeersContext & {
    dedupePeers?: boolean
    dependenciesTree: DependenciesTree<Pkg>
  }
): PeerId | undefined {
  if (typeof peerNodeId === 'string' && peerNodeId.startsWith('link:')) {
    return {
      name: alias,
      version: linkPathToPeerVersion(peerNodeId.slice(5)),
    }
  }
  if (ctx.dedupePeers) {
    // Use version-only peer identifiers instead of full dep paths.
    // This eliminates nested peer suffixes like (foo@1.0.0(bar@2.0.0)).
    const peerNode = ctx.dependenciesTree.get(peerNodeId)
    if (peerNode) {
      return peerIdOfResolvedPackage(peerNode.resolvedPackage)
    }
  }
  return ctx.pathsByNodeId.get(peerNodeId)
}
