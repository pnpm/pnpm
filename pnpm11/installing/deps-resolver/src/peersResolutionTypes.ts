import type {
  DepPath,
  PeerDependencyIssues,
  PkgIdWithPatchHash,
  ProjectRootDir,
} from '@pnpm/types'
import type { DeferredPromise } from 'p-defer'

import type { NodeId } from './nextNodeId.js'
import type {
  DependenciesTree,
  DependenciesTreeNode,
  PeerDependencies,
  ResolvedPackage,
} from './resolveDependencies.js'

export interface BaseGenericDependenciesGraphNode {
  // at this point the version is really needed only for logging
  modules: string
  dir: string
  depPath: DepPath
  depth: number
  peerDependencies?: PeerDependencies
  transitivePeerDependencies: Set<string>
  installable: boolean
  isBuilt?: boolean
  isPure: boolean
  resolvedPeerNames: Set<string>
  requiresBuild?: boolean
}

export interface GenericDependenciesGraphNode extends BaseGenericDependenciesGraphNode {
  childrenNodeIds: Record<string, NodeId>
}

export interface GenericDependenciesGraphNodeWithResolvedChildren extends BaseGenericDependenciesGraphNode {
  children: Record<string, DepPath>
}

export type PartialResolvedPackage = Pick<ResolvedPackage,
| 'id'
| 'pkgIdWithPatchHash'
| 'name'
| 'peerDependencies'
| 'version'
>

export interface GenericDependenciesGraph<Pkg extends PartialResolvedPackage> {
  [depPath: DepPath]: Pkg & GenericDependenciesGraphNode
}

export interface GenericDependenciesGraphWithResolvedChildren<Pkg extends PartialResolvedPackage> {
  [depPath: DepPath]: Pkg & GenericDependenciesGraphNodeWithResolvedChildren
}

export interface ProjectToResolve {
  directNodeIdsByAlias: Map<string, NodeId>
  // See PkgAddress.hoistedPeerProvider in resolveDependencies.ts
  hoistedPeerProviderNodeIds?: Set<NodeId>
  declaredDirectDependencies?: Set<string>
  explicitlyRequestedDirectDependencies?: Set<string>
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: Array<{ name: string, version: string, alias?: string }>
  rootDir: ProjectRootDir // is only needed for logging
  id: string
}

export type DependenciesByProjectId = Record<string, Map<string, DepPath>>

export interface MissingPeerInfo {
  range: string
  optional: boolean
}

export type MissingPeers = Map<string, MissingPeerInfo>

export interface PeersCacheItem {
  depPath: DeferredPromise<DepPath>
  resolvedPeers: Map<string, NodeId>
  missingPeers: MissingPeers
  // The node whose resolution created this entry and will resolve depPath.
  // See breakDepPathAwaitCycles.
  ownerNodeId: NodeId
}

export type PeersCache = Map<PkgIdWithPatchHash, PeersCacheItem[]>

export interface PeersResolution {
  missingPeers: MissingPeers
  resolvedPeers: Map<string, NodeId>
}

export interface ResolvePeersContext {
  pathsByNodeId: Map<NodeId, DepPath>
  pathsByNodeIdPromises: Map<NodeId, DeferredPromise<DepPath>>
  // The await edges between dep path calculations, consumed by
  // breakDepPathAwaitCycles: which pathsByNodeIdPromises entries each node's
  // calculateDepPath awaits, which node each peers-cache hit awaits, and
  // which promises were already resolved to `name@version` by cycle breaking.
  awaitedPeerNodeIdsByNodeId: Map<NodeId, Set<NodeId>>
  peersCacheOwnerByNodeId: Map<NodeId, NodeId>
  cycleBrokenNodeIds: Set<NodeId>
  depPathsByPkgId?: Map<PkgIdWithPatchHash, Set<DepPath>>
  nodeIdsByPreviousDepPath: Map<DepPath, NodeId>
  resolvedPeerProviderPaths?: Map<NodeId, DepPath>
  currentProviderSources: CurrentProviderSource[]
}

export interface ResolveNodePeersContext<Pkg extends PartialResolvedPackage> extends ResolvePeersContext {
  allPeerDepNames: Set<string>
  parentPkgsOfNode: ParentPkgsOfNode
  parentNodeIds: NodeId[]
  parentDepPathsChain: PkgIdWithPatchHash[]
  dependenciesTree: DependenciesTree<Pkg>
  depGraph: GenericDependenciesGraph<Pkg>
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
  peersCache: PeersCache
  purePkgs: Set<PkgIdWithPatchHash> // pure packages are those that don't rely on externally resolved peers
  dedupePeers?: boolean
  rootDir: ProjectRootDir
  lockfileDir: string
  peersSuffixMaxLength: number
}

export interface CurrentProviderSource {
  directNodeIdsByAlias: Map<string, NodeId>
  declaredDirectDependencies: Set<string>
  explicitlyRequestedDirectDependencies: Set<string>
}

export type CalculateDepPath = (cycles: string[][]) => Promise<void>
export type FinishingResolutionPromise = Promise<void>

export interface ParentPkgInfo {
  pkgIdWithPatchHash?: PkgIdWithPatchHash
  version?: string
  depth?: number
  occurrence?: number
}

export type ParentPkgsOfNode = Map<NodeId, Record<string, ParentPkgInfo>>

export interface PendingPeer {
  alias: string
  nodeId: NodeId
}

export interface ParentRefs {
  [name: string]: ParentRef
}

export interface ParentRef {
  version: string
  depth: number
  // this is null only for already installed top dependencies
  nodeId?: NodeId
  alias?: string
  occurrence: number
  parentNodeIds: NodeId[]
}

export interface ParentPkgNode<Pkg> {
  alias: string
  nodeId: NodeId
  node: DependenciesTreeNode<Pkg>
  parentNodeIds: NodeId[]
}
