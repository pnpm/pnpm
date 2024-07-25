import filenamify from 'filenamify'
import { analyzeGraph, type Graph } from 'graph-cycles'
import path from 'path'
import pDefer from 'p-defer'
import semver from 'semver'
import { semverUtils } from '@yarnpkg/core'
import {
  type DepPath,
  type ParentPackages,
  type PeerDependencyIssues,
  type PeerDependencyIssuesByProjects,
  type PkgIdWithPatchHash,
  type ProjectRootDir,
} from '@pnpm/types'
import { depPathToFilename, createPeersDirSuffix, type PeerId } from '@pnpm/dependency-path'
import partition from 'ramda/src/partition'
import pick from 'ramda/src/pick'
import { type NodeId } from './nextNodeId'
import {
  type ChildrenMap,
  type PeerDependencies,
  type DependenciesTree,
  type DependenciesTreeNode,
  type ResolvedPackage,
} from './resolveDependencies'
import { type ResolvedImporters } from './resolveDependencyTree'
import { mergePeers } from './mergePeers'
import { dedupeInjectedDeps } from './dedupeInjectedDeps'

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

export interface GenericDependenciesGraph<T extends PartialResolvedPackage> {
  [depPath: DepPath]: T & GenericDependenciesGraphNode
}

export interface GenericDependenciesGraphWithResolvedChildren<T extends PartialResolvedPackage> {
  [depPath: DepPath]: T & GenericDependenciesGraphNodeWithResolvedChildren
}

export interface ProjectToResolve {
  directNodeIdsByAlias: Map<string, NodeId>
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: Array<{ name: string, version: string, alias?: string }>
  rootDir: ProjectRootDir // is only needed for logging
  id: string
}

export type DependenciesByProjectId = Record<string, Map<string, DepPath>>

export async function resolvePeers<T extends PartialResolvedPackage> (
  opts: {
    allPeerDepNames: Set<string>
    projects: ProjectToResolve[]
    dependenciesTree: DependenciesTree<T>
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    lockfileDir: string
    resolvePeersFromWorkspaceRoot?: boolean
    dedupePeerDependents?: boolean
    dedupeInjectedDeps?: boolean
    resolvedImporters: ResolvedImporters
    peersSuffixMaxLength: number
  }
): Promise<{
    dependenciesGraph: GenericDependenciesGraphWithResolvedChildren<T>
    dependenciesByProjectId: DependenciesByProjectId
    peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  }> {
  const depGraph: GenericDependenciesGraph<T> = {}
  const pathsByNodeId = new Map<NodeId, DepPath>()
  const pathsByNodeIdPromises = new Map<NodeId, pDefer.DeferredPromise<DepPath>>()
  const depPathsByPkgId = new Map<PkgIdWithPatchHash, Set<DepPath>>()
  const _createPkgsByName = createPkgsByName.bind(null, opts.dependenciesTree)
  const rootPkgsByName = opts.resolvePeersFromWorkspaceRoot ? getRootPkgsByName(opts.dependenciesTree, opts.projects) : {}
  const peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects = {}

  const finishingList: FinishingResolutionPromise[] = []
  for (const { directNodeIdsByAlias, topParents, rootDir, id } of opts.projects) {
    const peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'> = { bad: {}, missing: {} }
    const pkgsByName = Object.fromEntries(Object.entries({
      ...rootPkgsByName,
      ..._createPkgsByName({ directNodeIdsByAlias, topParents }),
    }).filter(([peerName]) => opts.allPeerDepNames.has(peerName)))
    for (const { nodeId } of Object.values(pkgsByName)) {
      if (nodeId && !pathsByNodeIdPromises.has(nodeId)) {
        pathsByNodeIdPromises.set(nodeId, pDefer())
      }
    }

    // eslint-disable-next-line no-await-in-loop
    const { finishing } = await resolvePeersOfChildren(Object.fromEntries(directNodeIdsByAlias.entries()), pkgsByName, {
      allPeerDepNames: opts.allPeerDepNames,
      parentPkgsOfNode: new Map(),
      dependenciesTree: opts.dependenciesTree,
      depGraph,
      lockfileDir: opts.lockfileDir,
      parentNodeIds: [],
      parentDepPathsChain: [],
      pathsByNodeId,
      pathsByNodeIdPromises,
      depPathsByPkgId,
      peersCache: new Map(),
      peerDependencyIssues,
      purePkgs: new Set(),
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
      rootDir,
      virtualStoreDir: opts.virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    })
    if (finishing) {
      finishingList.push(finishing)
    }
    if (Object.keys(peerDependencyIssues.bad).length > 0 || Object.keys(peerDependencyIssues.missing).length > 0) {
      peerDependencyIssuesByProjects[id] = {
        ...peerDependencyIssues,
        ...mergePeers(peerDependencyIssues.missing),
      }
    }
  }
  await Promise.all(finishingList)

  const depGraphWithResolvedChildren = resolveChildren(depGraph)

  function resolveChildren<T extends PartialResolvedPackage> (depGraph: GenericDependenciesGraph<T>): GenericDependenciesGraphWithResolvedChildren<T> {
    Object.values(depGraph).forEach((node) => {
      node.children = {}
      for (const [alias, childNodeId] of Object.entries<NodeId>(node.childrenNodeIds)) {
        node.children[alias] = pathsByNodeId.get(childNodeId) ?? (childNodeId as unknown as DepPath)
      }
      delete node.childrenNodeIds
    })
    return depGraph as unknown as GenericDependenciesGraphWithResolvedChildren<T>
  }

  const dependenciesByProjectId: DependenciesByProjectId = {}
  for (const { directNodeIdsByAlias, id } of opts.projects) {
    dependenciesByProjectId[id] = new Map()
    for (const [alias, nodeId] of directNodeIdsByAlias.entries()) {
      dependenciesByProjectId[id].set(alias, pathsByNodeId.get(nodeId)!)
    }
  }
  if (opts.dedupeInjectedDeps) {
    dedupeInjectedDeps({
      dependenciesByProjectId,
      projects: opts.projects,
      depGraph: depGraphWithResolvedChildren,
      pathsByNodeId,
      lockfileDir: opts.lockfileDir,
      resolvedImporters: opts.resolvedImporters,
    })
  }
  if (opts.dedupePeerDependents) {
    const duplicates = Array.from(depPathsByPkgId.values()).filter((item) => item.size > 1)
    const allDepPathsMap = deduplicateAll(depGraphWithResolvedChildren, duplicates)
    for (const { id } of opts.projects) {
      for (const [alias, depPath] of dependenciesByProjectId[id].entries()) {
        dependenciesByProjectId[id].set(alias, allDepPathsMap[depPath] ?? depPath)
      }
    }
  }
  return {
    dependenciesGraph: depGraphWithResolvedChildren,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
  }
}

function nodeDepsCount (node: GenericDependenciesGraphNodeWithResolvedChildren): number {
  return Object.keys(node.children!).length + node.resolvedPeerNames.size
}

function deduplicateAll<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>,
  duplicates: Array<Set<DepPath>>
): Record<DepPath, DepPath> {
  const { depPathsMap, remainingDuplicates } = deduplicateDepPaths(duplicates, depGraph)
  if (remainingDuplicates.length === duplicates.length) {
    return depPathsMap
  }
  Object.values(depGraph).forEach((node) => {
    for (const [alias, childDepPath] of Object.entries<DepPath>(node.children)) {
      if (depPathsMap[childDepPath]) {
        node.children[alias] = depPathsMap[childDepPath]
      }
    }
  })
  if (Object.keys(depPathsMap).length > 0) {
    return {
      ...depPathsMap,
      ...deduplicateAll(depGraph, remainingDuplicates),
    }
  }
  return depPathsMap
}

interface DeduplicateDepPathsResult {
  depPathsMap: Record<DepPath, DepPath>
  remainingDuplicates: Array<Set<DepPath>>
}

function deduplicateDepPaths<T extends PartialResolvedPackage> (
  duplicates: Array<Set<DepPath>>,
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>
): DeduplicateDepPathsResult {
  const depCountSorter = (depPath1: DepPath, depPath2: DepPath) => nodeDepsCount(depGraph[depPath1]) - nodeDepsCount(depGraph[depPath2])
  const depPathsMap: Record<DepPath, DepPath> = {}
  const remainingDuplicates: Array<Set<DepPath>> = []

  for (const depPaths of duplicates) {
    const unresolvedDepPaths = new Set(depPaths.values())
    let currentDepPaths = [...depPaths].sort(depCountSorter)

    while (currentDepPaths.length) {
      const depPath1 = currentDepPaths.pop()!
      const nextDepPaths = []
      while (currentDepPaths.length) {
        const depPath2 = currentDepPaths.pop()!
        if (isCompatibleAndHasMoreDeps(depGraph, depPath1, depPath2)) {
          depPathsMap[depPath2] = depPath1
          unresolvedDepPaths.delete(depPath1)
          unresolvedDepPaths.delete(depPath2)
        } else {
          nextDepPaths.push(depPath2)
        }
      }
      nextDepPaths.push(...currentDepPaths)
      currentDepPaths = nextDepPaths.sort(depCountSorter)
    }

    if (unresolvedDepPaths.size) {
      remainingDuplicates.push(unresolvedDepPaths)
    }
  }
  return {
    depPathsMap,
    remainingDuplicates,
  }
}

function isCompatibleAndHasMoreDeps<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>,
  depPath1: DepPath,
  depPath2: DepPath
): boolean {
  const node1 = depGraph[depPath1]
  const node2 = depGraph[depPath2]
  if (nodeDepsCount(node1) < nodeDepsCount(node2)) return false

  const node1DepPathsSet = new Set(Object.values(node1.children!))
  const node2DepPaths = Object.values(node2.children!)
  if (!node2DepPaths.every((depPath) => node1DepPathsSet.has(depPath))) return false

  for (const depPath of node2.resolvedPeerNames) {
    if (!node1.resolvedPeerNames.has(depPath)) return false
  }
  return true
}

function getRootPkgsByName<T extends PartialResolvedPackage> (dependenciesTree: DependenciesTree<T>, projects: ProjectToResolve[]): ParentRefs {
  const rootProject = projects.length > 1 ? projects.find(({ id }) => id === '.') : null
  return rootProject == null ? {} : createPkgsByName(dependenciesTree, rootProject)
}

function createPkgsByName<T extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<T>,
  { directNodeIdsByAlias, topParents }: {
    directNodeIdsByAlias: Map<string, NodeId>
    topParents: Array<{ name: string, version: string, alias?: string, linkedDir?: string }>
  }
): ParentRefs {
  const parentRefs = toPkgByName(
    Array.from(directNodeIdsByAlias.entries())
      .map(([alias, nodeId]) => ({
        alias,
        node: dependenciesTree.get(nodeId)!,
        nodeId,
        parentNodeIds: [],
      }))
  )
  const _updateParentRefs = updateParentRefs.bind(null, parentRefs)
  for (const { name, version, alias, linkedDir } of topParents) {
    const pkg = {
      occurrence: 0,
      alias,
      depth: 0,
      version,
      nodeId: linkedDir as NodeId,
      parentNodeIds: [],
    }
    _updateParentRefs(name, pkg)
    if (alias && alias !== name) {
      _updateParentRefs(alias, pkg)
    }
  }
  return parentRefs
}

interface PeersCacheItem {
  depPath: pDefer.DeferredPromise<DepPath>
  resolvedPeers: Map<string, NodeId>
  missingPeers: Set<string>
}

type PeersCache = Map<PkgIdWithPatchHash, PeersCacheItem[]>

interface PeersResolution {
  missingPeers: Set<string>
  resolvedPeers: Map<string, NodeId>
}

interface ResolvePeersContext {
  pathsByNodeId: Map<NodeId, DepPath>
  pathsByNodeIdPromises: Map<NodeId, pDefer.DeferredPromise<DepPath>>
  depPathsByPkgId?: Map<PkgIdWithPatchHash, Set<DepPath>>
}

type CalculateDepPath = (cycles: NodeId[][]) => Promise<void>
type FinishingResolutionPromise = Promise<void>

interface ParentPkgInfo {
  pkgIdWithPatchHash?: PkgIdWithPatchHash
  version?: string
  depth?: number
  occurrence?: number
}

type ParentPkgsOfNode = Map<NodeId, Record<string, ParentPkgInfo>>

async function resolvePeersOfNode<T extends PartialResolvedPackage> (
  nodeId: NodeId,
  parentParentPkgs: ParentRefs,
  ctx: ResolvePeersContext & {
    allPeerDepNames: Set<string>
    parentPkgsOfNode: ParentPkgsOfNode
    parentNodeIds: NodeId[]
    parentDepPathsChain: PkgIdWithPatchHash[]
    dependenciesTree: DependenciesTree<T>
    depGraph: GenericDependenciesGraph<T>
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
    peersCache: PeersCache
    purePkgs: Set<PkgIdWithPatchHash> // pure packages are those that don't rely on externally resolved peers
    rootDir: ProjectRootDir
    lockfileDir: string
    peersSuffixMaxLength: number
  }
): Promise<PeersResolution & { finishing?: FinishingResolutionPromise, calculateDepPath?: CalculateDepPath }> {
  const node = ctx.dependenciesTree.get(nodeId)!
  if (node.depth === -1) return { resolvedPeers: new Map<string, NodeId>(), missingPeers: new Set<string>() }
  const resolvedPackage = node.resolvedPackage as T
  if (
    ctx.purePkgs.has(resolvedPackage.pkgIdWithPatchHash) &&
    ctx.depGraph[resolvedPackage.pkgIdWithPatchHash as unknown as DepPath].depth <= node.depth &&
    Object.keys(resolvedPackage.peerDependencies).length === 0
  ) {
    ctx.pathsByNodeId.set(nodeId, resolvedPackage.pkgIdWithPatchHash as unknown as DepPath)
    ctx.pathsByNodeIdPromises.get(nodeId)!.resolve(resolvedPackage.pkgIdWithPatchHash as unknown as DepPath)
    return { resolvedPeers: new Map<string, NodeId>(), missingPeers: new Set<string>() }
  }
  if (typeof node.children === 'function') {
    node.children = node.children()
  }
  const parentNodeIds = [...ctx.parentNodeIds, nodeId]
  const children = node.children
  let parentPkgs: ParentRefs
  if (Object.keys(children).length === 0) {
    parentPkgs = parentParentPkgs
  } else {
    parentPkgs = { ...parentParentPkgs }
    const parentPkgNodes: Array<ParentPkgNode<T>> = []
    for (const [alias, nodeId] of Object.entries(children)) {
      if (ctx.allPeerDepNames.has(alias)) {
        parentPkgNodes.push({
          alias,
          node: ctx.dependenciesTree.get(nodeId)!,
          nodeId,
          parentNodeIds,
        })
      }
    }
    const newParentPkgs = toPkgByName(parentPkgNodes)
    for (const [newParentPkgName, newParentPkg] of Object.entries(newParentPkgs)) {
      if (parentPkgs[newParentPkgName]) {
        if (parentPkgs[newParentPkgName].version !== newParentPkg.version) {
          newParentPkg.occurrence = parentPkgs[newParentPkgName].occurrence + 1
        }
        parentPkgs[newParentPkgName] = newParentPkg
      } else {
        parentPkgs[newParentPkgName] = newParentPkg
      }
    }
  }
  const hit = findHit(ctx, parentPkgs, resolvedPackage.pkgIdWithPatchHash)
  if (hit != null) {
    return {
      missingPeers: hit.missingPeers,
      finishing: (async () => {
        const depPath = await hit.depPath.promise
        ctx.pathsByNodeId.set(nodeId, depPath)
        ctx.depGraph[depPath].depth = Math.min(ctx.depGraph[depPath].depth, node.depth)
        ctx.pathsByNodeIdPromises.get(nodeId)!.resolve(depPath)
      })(),
      resolvedPeers: hit.resolvedPeers,
    }
  }

  const {
    resolvedPeers: unknownResolvedPeersOfChildren,
    missingPeers: missingPeersOfChildren,
    finishing,
  } = await resolvePeersOfChildren(children, parentPkgs, {
    ...ctx,
    parentNodeIds,
    parentDepPathsChain: ctx.parentDepPathsChain.includes(resolvedPackage.pkgIdWithPatchHash) ? ctx.parentDepPathsChain : [...ctx.parentDepPathsChain, resolvedPackage.pkgIdWithPatchHash],
  })

  const { resolvedPeers, missingPeers } = Object.keys(resolvedPackage.peerDependencies).length === 0
    ? { resolvedPeers: new Map<string, NodeId>(), missingPeers: new Set<string>() }
    : _resolvePeers({
      currentDepth: node.depth,
      dependenciesTree: ctx.dependenciesTree,
      lockfileDir: ctx.lockfileDir,
      nodeId,
      parentPkgs,
      peerDependencyIssues: ctx.peerDependencyIssues,
      resolvedPackage,
      rootDir: ctx.rootDir,
      parentNodeIds,
    })

  const allResolvedPeers = unknownResolvedPeersOfChildren
  for (const [k, v] of resolvedPeers) {
    allResolvedPeers.set(k, v)
  }
  allResolvedPeers.delete(node.resolvedPackage.name)

  const allMissingPeers = new Set<string>()
  for (const peer of missingPeersOfChildren) {
    allMissingPeers.add(peer)
  }
  for (const peer of missingPeers) {
    allMissingPeers.add(peer)
  }

  let cache: PeersCacheItem
  const isPure = allResolvedPeers.size === 0 && allMissingPeers.size === 0
  if (isPure) {
    ctx.purePkgs.add(resolvedPackage.pkgIdWithPatchHash)
  } else {
    cache = {
      missingPeers: allMissingPeers,
      depPath: pDefer(),
      resolvedPeers: allResolvedPeers,
    }
    if (ctx.peersCache.has(resolvedPackage.pkgIdWithPatchHash)) {
      ctx.peersCache.get(resolvedPackage.pkgIdWithPatchHash)!.push(cache)
    } else {
      ctx.peersCache.set(resolvedPackage.pkgIdWithPatchHash, [cache])
    }
  }

  let calculateDepPathIfNeeded: CalculateDepPath | undefined
  if (allResolvedPeers.size === 0) {
    addDepPathToGraph(resolvedPackage.pkgIdWithPatchHash as unknown as DepPath)
  } else {
    const peerIds: PeerId[] = []
    const pendingPeerNodeIds: NodeId[] = []
    for (const [alias, peerNodeId] of allResolvedPeers.entries()) {
      if (typeof peerNodeId === 'string' && peerNodeId.startsWith('link:')) {
        const linkedDir = peerNodeId.slice(5)
        peerIds.push({
          name: alias,
          version: filenamify(linkedDir, { replacement: '+' }),
        })
        continue
      }
      const peerDepPath = ctx.pathsByNodeId.get(peerNodeId)
      if (peerDepPath) {
        peerIds.push(peerDepPath)
        continue
      }
      pendingPeerNodeIds.push(peerNodeId)
    }
    if (pendingPeerNodeIds.length === 0) {
      const peersDirSuffix = createPeersDirSuffix(peerIds, ctx.peersSuffixMaxLength)
      addDepPathToGraph(`${resolvedPackage.pkgIdWithPatchHash}${peersDirSuffix}` as DepPath)
    } else {
      calculateDepPathIfNeeded = calculateDepPath.bind(null, peerIds, pendingPeerNodeIds)
    }
  }

  return {
    resolvedPeers: allResolvedPeers,
    missingPeers: allMissingPeers,
    calculateDepPath: calculateDepPathIfNeeded,
    finishing,
  }

  async function calculateDepPath (
    peerIds: PeerId[],
    pendingPeerNodeIds: NodeId[],
    cycles: NodeId[][]
  ): Promise<void> {
    const cyclicPeerNodeIds = new Set()
    for (const cycle of cycles) {
      if (cycle.includes(nodeId)) {
        for (const peerNodeId of cycle) {
          cyclicPeerNodeIds.add(peerNodeId)
        }
      }
    }
    const peersDirSuffix = createPeersDirSuffix([
      ...peerIds,
      ...await Promise.all(pendingPeerNodeIds
        .map(async (peerNodeId) => {
          if (cyclicPeerNodeIds.has(peerNodeId)) {
            const { name, version } = (ctx.dependenciesTree.get(peerNodeId)!.resolvedPackage as T)
            return `${name}@${version}`
          }
          return ctx.pathsByNodeIdPromises.get(peerNodeId)!.promise
        })
      ),
    ], ctx.peersSuffixMaxLength)
    addDepPathToGraph(`${resolvedPackage.pkgIdWithPatchHash}${peersDirSuffix}` as DepPath)
  }

  function addDepPathToGraph (depPath: DepPath): void {
    cache?.depPath.resolve(depPath)
    ctx.pathsByNodeId.set(nodeId, depPath)
    ctx.pathsByNodeIdPromises.get(nodeId)!.resolve(depPath)
    if (ctx.depPathsByPkgId != null) {
      if (!ctx.depPathsByPkgId.has(resolvedPackage.pkgIdWithPatchHash)) {
        ctx.depPathsByPkgId.set(resolvedPackage.pkgIdWithPatchHash, new Set([depPath]))
      } else {
        ctx.depPathsByPkgId.get(resolvedPackage.pkgIdWithPatchHash)!.add(depPath)
      }
    }
    const peerDependencies = { ...resolvedPackage.peerDependencies }
    if (!ctx.depGraph[depPath] || ctx.depGraph[depPath].depth > node.depth) {
      const modules = path.join(ctx.virtualStoreDir, depPathToFilename(depPath, ctx.virtualStoreDirMaxLength), 'node_modules')
      const dir = path.join(modules, resolvedPackage.name)

      const transitivePeerDependencies = new Set<string>()
      for (const unknownPeer of allResolvedPeers.keys()) {
        if (!peerDependencies[unknownPeer]) {
          transitivePeerDependencies.add(unknownPeer)
        }
      }
      for (const unknownPeer of missingPeersOfChildren) {
        if (!peerDependencies[unknownPeer]) {
          transitivePeerDependencies.add(unknownPeer)
        }
      }
      ctx.depGraph[depPath] = {
        ...(node.resolvedPackage as T),
        childrenNodeIds: Object.assign(
          getPreviouslyResolvedChildren(ctx, (node.resolvedPackage as T).pkgIdWithPatchHash),
          children,
          Object.fromEntries(resolvedPeers.entries())
        ),
        depPath,
        depth: node.depth,
        dir,
        installable: node.installable,
        isPure,
        modules,
        peerDependencies,
        transitivePeerDependencies,
        resolvedPeerNames: new Set(allResolvedPeers.keys()),
      }
    }
  }
}

function findHit<T extends PartialResolvedPackage> (ctx: {
  parentPkgsOfNode: ParentPkgsOfNode
  peersCache: PeersCache
  purePkgs: Set<PkgIdWithPatchHash>
  pathsByNodeId: Map<NodeId, DepPath>
  dependenciesTree: DependenciesTree<T>
}, parentPkgs: ParentRefs, pkgIdWithPatchHash: PkgIdWithPatchHash) {
  const cacheItems = ctx.peersCache.get(pkgIdWithPatchHash)
  if (!cacheItems) return undefined
  return cacheItems.find((cache) => {
    for (const [name, cachedNodeId] of cache.resolvedPeers) {
      const parentPkgNodeId = parentPkgs[name]?.nodeId
      if (Boolean(parentPkgNodeId) !== Boolean(cachedNodeId)) return false
      if (parentPkgNodeId === cachedNodeId) continue
      if (!parentPkgNodeId) return false
      if (
        ctx.pathsByNodeId.has(cachedNodeId) &&
        ctx.pathsByNodeId.get(cachedNodeId) === ctx.pathsByNodeId.get(parentPkgNodeId)
      ) continue
      if (!ctx.dependenciesTree.has(parentPkgNodeId) && typeof parentPkgNodeId === 'string' && parentPkgNodeId.startsWith('link:')) {
        return false
      }
      const parentPkgId = (ctx.dependenciesTree.get(parentPkgNodeId)!.resolvedPackage as T).pkgIdWithPatchHash
      const cachedPkgId = (ctx.dependenciesTree.get(cachedNodeId)!.resolvedPackage as T).pkgIdWithPatchHash
      if (parentPkgId !== cachedPkgId) {
        return false
      }
      if (
        !ctx.purePkgs.has(parentPkgId) &&
        !parentPackagesMatch(ctx, cachedNodeId, parentPkgNodeId)
      ) {
        return false
      }
    }
    for (const missingPeer of cache.missingPeers) {
      if (parentPkgs[missingPeer]) return false
    }
    return true
  })
}

function parentPackagesMatch (ctx: {
  parentPkgsOfNode: ParentPkgsOfNode
  purePkgs: Set<PkgIdWithPatchHash>
}, cachedNodeId: NodeId, checkedNodeId: NodeId): boolean {
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
    Object.entries(cachedParentPkgs).every(([name, { version, pkgIdWithPatchHash }]) => {
      if (checkedParentPkgs[name] == null) return false
      if (version && checkedParentPkgs[name].version) {
        return version === checkedParentPkgs[name].version
      }
      return pkgIdWithPatchHash != null &&
        (pkgIdWithPatchHash === checkedParentPkgs[name].pkgIdWithPatchHash) &&
        (
          peerDepsAreNotShadowed ||
          // Peer dependencies that appear last we can consider valid.
          // If they do depend on other peer dependencies then they must be those that we will check further.
          checkedParentPkgs[name].depth === maxDepth ||
          ctx.purePkgs.has(pkgIdWithPatchHash)
        )
    })
  )
}

function parentPkgsHaveSingleOccurrence (parentPkgs: Record<string, ParentPkgInfo>): boolean {
  return Object.values(parentPkgs).every(({ occurrence }) => occurrence === 0 || occurrence == null)
}

// When a package has itself in the subdependencies, so there's a cycle,
// pnpm will break the cycle, when it first repeats itself.
// However, when the cycle is broken up, the last repeated package is removed
// from the dependencies of the parent package.
// So we need to merge all the children of all the parent packages with same ID as the resolved package.
// This way we get all the children that were removed, when ending cycles.
function getPreviouslyResolvedChildren<T extends PartialResolvedPackage> (
  {
    parentNodeIds,
    parentDepPathsChain,
    dependenciesTree,
  }: {
    parentNodeIds: NodeId[]
    parentDepPathsChain: PkgIdWithPatchHash[]
    dependenciesTree: DependenciesTree<T>
  },
  currentDepPath: PkgIdWithPatchHash
): ChildrenMap {
  const allChildren: ChildrenMap = {}

  if (!currentDepPath || !parentDepPathsChain.includes(currentDepPath)) return allChildren

  for (let i = parentNodeIds.length - 1; i >= 0; i--) {
    const parentNode = dependenciesTree.get(parentNodeIds[i])!
    if ((parentNode.resolvedPackage as T).pkgIdWithPatchHash === currentDepPath) {
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

async function resolvePeersOfChildren<T extends PartialResolvedPackage> (
  children: {
    [alias: string]: NodeId
  },
  parentPkgs: ParentRefs,
  ctx: ResolvePeersContext & {
    allPeerDepNames: Set<string>
    parentPkgsOfNode: ParentPkgsOfNode
    parentNodeIds: NodeId[]
    parentDepPathsChain: PkgIdWithPatchHash[]
    peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
    peersCache: PeersCache
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    purePkgs: Set<PkgIdWithPatchHash>
    depGraph: GenericDependenciesGraph<T>
    dependenciesTree: DependenciesTree<T>
    rootDir: ProjectRootDir
    lockfileDir: string
    peersSuffixMaxLength: number
  }
): Promise<PeersResolution & { finishing: Promise<void> }> {
  const allResolvedPeers = new Map<string, NodeId>()
  const allMissingPeers = new Set<string>()

  // Partition children based on whether they're repeated in parentPkgs.
  // This impacts the efficiency of graph traversal and prevents potential out-of-memory errors.
  // We check repeated first as the peers resolution of those probably are cached already.
  const [repeated, notRepeated] = partition(([alias]) => parentPkgs[alias] != null, Object.entries(children))
  const nodeIds = Array.from(new Set([...repeated, ...notRepeated].map(([, nodeId]) => nodeId)))

  for (const nodeId of nodeIds) {
    if (!ctx.pathsByNodeIdPromises.has(nodeId)) {
      ctx.pathsByNodeIdPromises.set(nodeId, pDefer())
    }
  }

  // Resolving non-repeated nodes before repeated nodes proved to be slightly faster.
  const calculateDepPaths: CalculateDepPath[] = []
  const graph = []
  const finishingList: FinishingResolutionPromise[] = []
  const parentDepPaths: Record<string, ParentPkgInfo> = {}
  for (const [name, parentPkg] of Object.entries(parentPkgs)) {
    if (!ctx.allPeerDepNames.has(name)) continue
    if (parentPkg.nodeId && (typeof parentPkg.nodeId === 'number' || !parentPkg.nodeId.startsWith('link:'))) {
      parentDepPaths[name] = {
        pkgIdWithPatchHash: (ctx.dependenciesTree.get(parentPkg.nodeId)!.resolvedPackage as T).pkgIdWithPatchHash,
        depth: parentPkg.depth,
        occurrence: parentPkg.occurrence,
      }
    } else {
      parentDepPaths[name] = { version: parentPkg.version }
    }
  }
  for (const childNodeId of nodeIds) {
    ctx.parentPkgsOfNode.set(childNodeId, parentDepPaths)
  }
  for (const childNodeId of nodeIds) {
    const {
      resolvedPeers,
      missingPeers,
      calculateDepPath,
      finishing,
    } = await resolvePeersOfNode(childNodeId, parentPkgs, ctx) // eslint-disable-line no-await-in-loop
    if (finishing) {
      finishingList.push(finishing)
    }
    if (calculateDepPath) {
      calculateDepPaths.push(calculateDepPath)
    }
    const edges = []
    for (const [peerName, peerNodeId] of resolvedPeers) {
      allResolvedPeers.set(peerName, peerNodeId)
      edges.push(peerNodeId)
    }
    graph.push([childNodeId, edges])
    for (const missingPeer of missingPeers) {
      allMissingPeers.add(missingPeer)
    }
  }
  if (calculateDepPaths.length) {
    const { cycles } = analyzeGraph(graph as unknown as Graph) as unknown as { cycles: NodeId[][] }
    finishingList.push(...calculateDepPaths.map((calculateDepPath) => calculateDepPath(cycles)))
  }
  const finishing = Promise.all(finishingList).then(() => {})

  const unknownResolvedPeersOfChildren = new Map<string, NodeId>()
  for (const [alias, v] of allResolvedPeers) {
    if (!children[alias]) {
      unknownResolvedPeersOfChildren.set(alias, v)
    }
  }

  return { resolvedPeers: unknownResolvedPeersOfChildren, missingPeers: allMissingPeers, finishing }
}

function _resolvePeers<T extends PartialResolvedPackage> (
  ctx: {
    currentDepth: number
    lockfileDir: string
    nodeId: NodeId
    parentPkgs: ParentRefs
    parentNodeIds: NodeId[]
    resolvedPackage: T
    dependenciesTree: DependenciesTree<T>
    rootDir: ProjectRootDir
    peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
  }
): PeersResolution {
  const resolvedPeers = new Map<string, NodeId>()
  const missingPeers = new Set<string>()
  for (const [peerName, { version, optional }] of Object.entries(ctx.resolvedPackage.peerDependencies)) {
    const peerVersionRange = version.replace(/^workspace:/, '')

    const resolved = ctx.parentPkgs[peerName]
    const optionalPeer = optional === true

    if (!resolved) {
      missingPeers.add(peerName)
      const location = getLocationFromParentNodeIds(ctx)
      if (!ctx.peerDependencyIssues.missing[peerName]) {
        ctx.peerDependencyIssues.missing[peerName] = []
      }
      ctx.peerDependencyIssues.missing[peerName].push({
        parents: location.parents,
        optional: optionalPeer,
        wantedRange: peerVersionRange,
      })
      continue
    }

    if (!semverUtils.satisfiesWithPrereleases(resolved.version, peerVersionRange, true)) {
      const location = getLocationFromParentNodeIds(ctx)
      if (!ctx.peerDependencyIssues.bad[peerName]) {
        ctx.peerDependencyIssues.bad[peerName] = []
      }
      const peerLocation = resolved.nodeId == null
        ? []
        : getLocationFromParentNodeIds({
          dependenciesTree: ctx.dependenciesTree,
          parentNodeIds: resolved.parentNodeIds,
        }).parents
      ctx.peerDependencyIssues.bad[peerName].push({
        foundVersion: resolved.version,
        resolvedFrom: peerLocation,
        parents: location.parents,
        optional: optionalPeer,
        wantedRange: peerVersionRange,
      })
    }

    if (resolved?.nodeId) resolvedPeers.set(peerName, resolved.nodeId)
  }
  return { resolvedPeers, missingPeers }
}

interface Location {
  projectId: string
  parents: ParentPackages
}

function getLocationFromParentNodeIds<T> (
  {
    dependenciesTree,
    parentNodeIds,
  }: {
    dependenciesTree: DependenciesTree<T>
    parentNodeIds: NodeId[]
  }
): Location {
  const parents = parentNodeIds
    .map((nid) => pick(['name', 'version'], dependenciesTree.get(nid)!.resolvedPackage as ResolvedPackage))
  return {
    projectId: '.',
    parents,
  }
}

interface ParentRefs {
  [name: string]: ParentRef
}

interface ParentRef {
  version: string
  depth: number
  // this is null only for already installed top dependencies
  nodeId?: NodeId
  alias?: string
  occurrence: number
  parentNodeIds: NodeId[]
}

interface ParentPkgNode<T> {
  alias: string
  nodeId: NodeId
  node: DependenciesTreeNode<T>
  parentNodeIds: NodeId[]
}

function toPkgByName<T extends PartialResolvedPackage> (nodes: Array<ParentPkgNode<T>>): ParentRefs {
  const pkgsByName: ParentRefs = {}
  const _updateParentRefs = updateParentRefs.bind(null, pkgsByName)
  for (const { alias, node, nodeId, parentNodeIds } of nodes) {
    const pkg = {
      alias,
      depth: node.depth,
      nodeId,
      version: node.resolvedPackage.version,
      occurrence: 0,
      parentNodeIds,
    }
    _updateParentRefs(alias, pkg)
    if (alias !== node.resolvedPackage.name) {
      _updateParentRefs(node.resolvedPackage.name, pkg)
    }
  }
  return pkgsByName
}

function updateParentRefs (parentRefs: ParentRefs, newAlias: string, pkg: ParentRef): void {
  const existing = parentRefs[newAlias]
  if (existing) {
    const existingHasAlias = existing.alias != null && existing.alias !== newAlias
    if (!existingHasAlias) return
    const newHasAlias = pkg.alias != null && pkg.alias !== newAlias
    if (newHasAlias && semver.gte(existing.version, pkg.version)) return
  }
  parentRefs[newAlias] = pkg
}
