import filenamify from 'filenamify'
import path from 'path'
import semver from 'semver'
import { semverUtils } from '@yarnpkg/core'
import type {
  Dependencies,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
} from '@pnpm/types'
import { depPathToFilename, createPeersFolderSuffix } from '@pnpm/dependency-path'
import mapValues from 'ramda/src/map'
import pick from 'ramda/src/pick'
import pickBy from 'ramda/src/pickBy'
import scan from 'ramda/src/scan'
import {
  type DependenciesTree,
  type DependenciesTreeNode,
  type ResolvedPackage,
} from './resolveDependencies'
import { mergePeers } from './mergePeers'
import { createNodeId, splitNodeId } from './nodeIdUtils'

export interface GenericDependenciesGraphNode {
  // at this point the version is really needed only for logging
  modules: string
  dir: string
  children: Record<string, string>
  depth: number
  peerDependencies?: Dependencies
  transitivePeerDependencies: Set<string>
  installable: boolean
  isBuilt?: boolean
  isPure: boolean
  resolvedPeerNames: string[]
}

export type PartialResolvedPackage = Pick<ResolvedPackage,
| 'depPath'
| 'name'
| 'peerDependencies'
| 'peerDependenciesMeta'
| 'version'
>

export interface GenericDependenciesGraph<T extends PartialResolvedPackage> {
  [depPath: string]: T & GenericDependenciesGraphNode
}

export interface ProjectToResolve {
  directNodeIdsByAlias: { [alias: string]: string }
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: Array<{ name: string, version: string, alias?: string }>
  rootDir: string // is only needed for logging
  id: string
}

export function resolvePeers<T extends PartialResolvedPackage> (
  opts: {
    projects: ProjectToResolve[]
    dependenciesTree: DependenciesTree<T>
    virtualStoreDir: string
    lockfileDir: string
    resolvePeersFromWorkspaceRoot?: boolean
    dedupePeerDependents?: boolean
  }
): {
    dependenciesGraph: GenericDependenciesGraph<T>
    dependenciesByProjectId: { [id: string]: { [alias: string]: string } }
    peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  } {
  const depGraph: GenericDependenciesGraph<T> = {}
  const pathsByNodeId: Record<string, string> = {}
  const depPathsByPkgId: Record<string, string[]> = {}
  const _createPkgsByName = createPkgsByName.bind(null, opts.dependenciesTree)
  const rootPkgsByName = opts.resolvePeersFromWorkspaceRoot ? getRootPkgsByName(opts.dependenciesTree, opts.projects) : {}
  const peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects = {}

  for (const { directNodeIdsByAlias, topParents, rootDir, id } of opts.projects) {
    const peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'> = { bad: {}, missing: {} }
    const pkgsByName = {
      ...rootPkgsByName,
      ..._createPkgsByName({ directNodeIdsByAlias, topParents }),
    }

    resolvePeersOfChildren(directNodeIdsByAlias, pkgsByName, {
      dependenciesTree: opts.dependenciesTree,
      depGraph,
      lockfileDir: opts.lockfileDir,
      pathsByNodeId,
      depPathsByPkgId,
      peersCache: new Map(),
      peerDependencyIssues,
      purePkgs: new Set(),
      rootDir,
      virtualStoreDir: opts.virtualStoreDir,
    })
    if (Object.keys(peerDependencyIssues.bad).length > 0 || Object.keys(peerDependencyIssues.missing).length > 0) {
      peerDependencyIssuesByProjects[id] = {
        ...peerDependencyIssues,
        ...mergePeers(peerDependencyIssues.missing),
      }
    }
  }

  Object.values(depGraph).forEach((node) => {
    node.children = mapValues((childNodeId) => pathsByNodeId[childNodeId] ?? childNodeId, node.children)
  })

  const dependenciesByProjectId: { [id: string]: Record<string, string> } = {}
  for (const { directNodeIdsByAlias, id } of opts.projects) {
    dependenciesByProjectId[id] = mapValues((nodeId) => pathsByNodeId[nodeId], directNodeIdsByAlias)
  }
  if (opts.dedupePeerDependents) {
    const duplicates = Object.values(depPathsByPkgId).filter((item) => item.length > 1)
    const allDepPathsMap = deduplicateAll(depGraph, duplicates)
    for (const { id } of opts.projects) {
      dependenciesByProjectId[id] = mapValues((depPath) => allDepPathsMap[depPath] ?? depPath, dependenciesByProjectId[id])
    }
  }
  return {
    dependenciesGraph: depGraph,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
  }
}

function nodeDepsCount (node: GenericDependenciesGraphNode) {
  return Object.keys(node.children).length + node.resolvedPeerNames.length
}

function deduplicateAll<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraph<T>,
  duplicates: string[][]
): Record<string, string> {
  const { depPathsMap, remainingDuplicates } = deduplicateDepPaths(duplicates, depGraph)
  if (remainingDuplicates.length === duplicates.length) {
    return depPathsMap
  }
  Object.values(depGraph).forEach((node) => {
    node.children = mapValues((childDepPath) => depPathsMap[childDepPath] ?? childDepPath, node.children)
  })
  if (Object.keys(depPathsMap).length > 0) {
    return {
      ...depPathsMap,
      ...deduplicateAll(depGraph, remainingDuplicates),
    }
  }
  return depPathsMap
}

function deduplicateDepPaths<T extends PartialResolvedPackage> (
  duplicates: string[][],
  depGraph: GenericDependenciesGraph<T>
) {
  const depCountSorter = (depPath1: string, depPath2: string) => nodeDepsCount(depGraph[depPath1]) - nodeDepsCount(depGraph[depPath2])
  const depPathsMap: Record<string, string> = {}
  const remainingDuplicates: string[][] = []

  for (const depPaths of duplicates) {
    const unresolvedDepPaths = new Set(depPaths)
    let currentDepPaths = depPaths.sort(depCountSorter)

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
      remainingDuplicates.push([...unresolvedDepPaths])
    }
  }
  return {
    depPathsMap,
    remainingDuplicates,
  }
}

function isCompatibleAndHasMoreDeps<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraph<T>,
  depPath1: string,
  depPath2: string
) {
  const node1 = depGraph[depPath1]
  const node2 = depGraph[depPath2]
  const node1DepPaths = Object.values(node1.children)
  const node2DepPaths = Object.values(node2.children)
  return nodeDepsCount(node1) > nodeDepsCount(node2) &&
    node2DepPaths.every((depPath) => node1DepPaths.includes(depPath)) &&
    node2.resolvedPeerNames.every((depPath) => node1.resolvedPeerNames.includes(depPath))
}

function getRootPkgsByName<T extends PartialResolvedPackage> (dependenciesTree: DependenciesTree<T>, projects: ProjectToResolve[]) {
  const rootProject = projects.length > 1 ? projects.find(({ id }) => id === '.') : null
  return rootProject == null ? {} : createPkgsByName(dependenciesTree, rootProject)
}

function createPkgsByName<T extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<T>,
  { directNodeIdsByAlias, topParents }: {
    directNodeIdsByAlias: { [alias: string]: string }
    topParents: Array<{ name: string, version: string, alias?: string, linkedDir?: string }>
  }
) {
  const parentRefs = toPkgByName(
    Object
      .keys(directNodeIdsByAlias)
      .map((alias) => ({
        alias,
        node: dependenciesTree[directNodeIdsByAlias[alias]],
        nodeId: directNodeIdsByAlias[alias],
      }))
  )
  const _updateParentRefs = updateParentRefs.bind(null, parentRefs)
  for (const { name, version, alias, linkedDir } of topParents) {
    const pkg = {
      alias,
      depth: 0,
      version,
      nodeId: linkedDir,
    }
    _updateParentRefs(name, pkg)
    if (alias && alias !== name) {
      _updateParentRefs(alias, pkg)
    }
  }
  return parentRefs
}

interface PeersCacheItem {
  depPath: string
  resolvedPeers: Array<[string, string]>
  missingPeers: string[]
}

type PeersCache = Map<string, PeersCacheItem[]>

interface PeersResolution {
  missingPeers: string[]
  resolvedPeers: Record<string, string>
}

interface ResolvePeersContext {
  pathsByNodeId: { [nodeId: string]: string }
  depPathsByPkgId?: Record<string, string[]>
}

function resolvePeersOfNode<T extends PartialResolvedPackage> (
  nodeId: string,
  parentParentPkgs: ParentRefs,
  ctx: ResolvePeersContext & {
    dependenciesTree: DependenciesTree<T>
    depGraph: GenericDependenciesGraph<T>
    virtualStoreDir: string
    peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
    peersCache: PeersCache
    purePkgs: Set<string> // pure packages are those that don't rely on externally resolved peers
    rootDir: string
    lockfileDir: string
  }
): PeersResolution {
  const node = ctx.dependenciesTree[nodeId]
  if (node.depth === -1) return { resolvedPeers: {}, missingPeers: [] }
  const resolvedPackage = node.resolvedPackage as T
  if (
    ctx.purePkgs.has(resolvedPackage.depPath) &&
    ctx.depGraph[resolvedPackage.depPath].depth <= node.depth &&
    Object.keys(resolvedPackage.peerDependencies).length === 0
  ) {
    ctx.pathsByNodeId[nodeId] = resolvedPackage.depPath
    return { resolvedPeers: {}, missingPeers: [] }
  }
  if (typeof node.children === 'function') {
    node.children = node.children()
  }
  const children = node.children
  const parentPkgs = Object.keys(children).length === 0
    ? parentParentPkgs
    : Object.assign(
      Object.create(parentParentPkgs),
      toPkgByName(
        Object.entries(children).map(([alias, nodeId]) => ({
          alias,
          node: ctx.dependenciesTree[nodeId],
          nodeId,
        }))
      )
    )
  const hit = ctx.peersCache.get(resolvedPackage.depPath)?.find((cache) =>
    cache.resolvedPeers
      .every(([name, cachedNodeId]) => {
        const parentPkgNodeId = parentPkgs[name]?.nodeId
        if (!parentPkgNodeId || !cachedNodeId) return false
        if (parentPkgNodeId === cachedNodeId) return true
        if (
          ctx.pathsByNodeId[cachedNodeId] &&
          ctx.pathsByNodeId[cachedNodeId] === ctx.pathsByNodeId[parentPkgNodeId]
        ) return true
        if (!ctx.dependenciesTree[parentPkgNodeId] && parentPkgNodeId.startsWith('link:')) {
          return false
        }
        const parentDepPath = (ctx.dependenciesTree[parentPkgNodeId].resolvedPackage as T).depPath
        if (!ctx.purePkgs.has(parentDepPath)) return false
        const cachedDepPath = (ctx.dependenciesTree[cachedNodeId].resolvedPackage as T).depPath
        return parentDepPath === cachedDepPath
      }) && cache.missingPeers.every((missingPeer) => !parentPkgs[missingPeer])
  )
  if (hit != null) {
    ctx.pathsByNodeId[nodeId] = hit.depPath
    ctx.depGraph[hit.depPath].depth = Math.min(ctx.depGraph[hit.depPath].depth, node.depth)
    return {
      missingPeers: hit.missingPeers,
      resolvedPeers: Object.fromEntries(hit.resolvedPeers),
    }
  }

  const {
    resolvedPeers: unknownResolvedPeersOfChildren,
    missingPeers: missingPeersOfChildren,
  } = resolvePeersOfChildren(children, parentPkgs, ctx)

  const { resolvedPeers, missingPeers } = Object.keys(resolvedPackage.peerDependencies).length === 0
    ? { resolvedPeers: {}, missingPeers: [] }
    : _resolvePeers({
      currentDepth: node.depth,
      dependenciesTree: ctx.dependenciesTree,
      lockfileDir: ctx.lockfileDir,
      nodeId,
      parentPkgs,
      peerDependencyIssues: ctx.peerDependencyIssues,
      resolvedPackage,
      rootDir: ctx.rootDir,
    })

  const allResolvedPeers = Object.assign(unknownResolvedPeersOfChildren, resolvedPeers)
  delete allResolvedPeers[node.resolvedPackage.name]
  const allMissingPeers = Array.from(new Set([...missingPeersOfChildren, ...missingPeers]))

  let depPath: string
  if (Object.keys(allResolvedPeers).length === 0) {
    depPath = resolvedPackage.depPath
  } else {
    const peersFolderSuffix = createPeersFolderSuffix(
      Object.entries(allResolvedPeers)
        .map(([alias, nodeId]) => {
          if (nodeId.startsWith('link:')) {
            const linkedDir = nodeId.slice(5)
            return {
              name: alias,
              version: filenamify(linkedDir, { replacement: '+' }),
            }
          }
          const { name, version } = ctx.dependenciesTree[nodeId].resolvedPackage
          return { name, version }
        })
    )
    depPath = `${resolvedPackage.depPath}${peersFolderSuffix}`
  }
  const localLocation = path.join(ctx.virtualStoreDir, depPathToFilename(depPath))
  const modules = path.join(localLocation, 'node_modules')
  const isPure = Object.keys(allResolvedPeers).length === 0 && allMissingPeers.length === 0
  if (isPure) {
    ctx.purePkgs.add(resolvedPackage.depPath)
  } else {
    const cache = {
      missingPeers: allMissingPeers,
      depPath,
      resolvedPeers: Object.entries(allResolvedPeers),
    }
    if (ctx.peersCache.has(resolvedPackage.depPath)) {
      ctx.peersCache.get(resolvedPackage.depPath)!.push(cache)
    } else {
      ctx.peersCache.set(resolvedPackage.depPath, [cache])
    }
  }

  ctx.pathsByNodeId[nodeId] = depPath
  if (ctx.depPathsByPkgId != null) {
    if (!ctx.depPathsByPkgId[resolvedPackage.depPath]) {
      ctx.depPathsByPkgId[resolvedPackage.depPath] = []
    }
    if (!ctx.depPathsByPkgId[resolvedPackage.depPath].includes(depPath)) {
      ctx.depPathsByPkgId[resolvedPackage.depPath].push(depPath)
    }
  }
  const peerDependencies = { ...resolvedPackage.peerDependencies }
  if (!ctx.depGraph[depPath] || ctx.depGraph[depPath].depth > node.depth) {
    const dir = path.join(modules, resolvedPackage.name)

    const transitivePeerDependencies = new Set<string>()
    const unknownPeers = [
      ...Object.keys(unknownResolvedPeersOfChildren),
      ...missingPeersOfChildren,
    ]
    if (unknownPeers.length > 0) {
      for (const unknownPeer of unknownPeers) {
        if (!peerDependencies[unknownPeer]) {
          transitivePeerDependencies.add(unknownPeer)
        }
      }
    }
    ctx.depGraph[depPath] = {
      ...(node.resolvedPackage as T),
      children: Object.assign(
        getPreviouslyResolvedChildren(nodeId, ctx.dependenciesTree),
        children,
        resolvedPeers
      ),
      depPath,
      depth: node.depth,
      dir,
      installable: node.installable,
      isPure,
      modules,
      peerDependencies,
      transitivePeerDependencies,
      resolvedPeerNames: Object.keys(allResolvedPeers),
    }
  }
  return { resolvedPeers: allResolvedPeers, missingPeers: allMissingPeers }
}

// When a package has itself in the subdependencies, so there's a cycle,
// pnpm will break the cycle, when it first repeats itself.
// However, when the cycle is broken up, the last repeated package is removed
// from the dependencies of the parent package.
// So we need to merge all the children of all the parent packages with same ID as the resolved package.
// This way we get all the children that were removed, when ending cycles.
function getPreviouslyResolvedChildren<T extends PartialResolvedPackage> (nodeId: string, dependenciesTree: DependenciesTree<T>) {
  const parentIds = splitNodeId(nodeId)
  const ownId = parentIds.pop()
  const allChildren = {}

  if (!ownId || !parentIds.includes(ownId)) return allChildren

  const nodeIdChunks = parentIds.join('>').split(`>${ownId}>`)
  nodeIdChunks.pop()
  nodeIdChunks.reduce((accNodeId, part) => {
    accNodeId += `>${part}>${ownId}`
    const parentNode = dependenciesTree[`${accNodeId}>`]
    if (typeof parentNode.children === 'function') {
      parentNode.children = parentNode.children()
    }
    Object.assign(
      allChildren,
      parentNode.children
    )
    return accNodeId
  }, '')
  return allChildren
}

function resolvePeersOfChildren<T extends PartialResolvedPackage> (
  children: {
    [alias: string]: string
  },
  parentPkgs: ParentRefs,
  ctx: ResolvePeersContext & {
    peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
    peersCache: PeersCache
    virtualStoreDir: string
    purePkgs: Set<string>
    depGraph: GenericDependenciesGraph<T>
    dependenciesTree: DependenciesTree<T>
    rootDir: string
    lockfileDir: string
  }
): PeersResolution {
  const allResolvedPeers: Record<string, string> = {}
  const allMissingPeers = new Set<string>()

  for (const childNodeId of Object.values(children)) {
    const { resolvedPeers, missingPeers } = resolvePeersOfNode(childNodeId, parentPkgs, ctx)
    Object.assign(allResolvedPeers, resolvedPeers)
    missingPeers.forEach((missingPeer) => allMissingPeers.add(missingPeer))
  }

  const unknownResolvedPeersOfChildren: Record<string, string> = pickBy((_, alias) => !children[alias], allResolvedPeers)

  return { resolvedPeers: unknownResolvedPeersOfChildren, missingPeers: Array.from(allMissingPeers) }
}

function _resolvePeers<T extends PartialResolvedPackage> (
  ctx: {
    currentDepth: number
    lockfileDir: string
    nodeId: string
    parentPkgs: ParentRefs
    resolvedPackage: T
    dependenciesTree: DependenciesTree<T>
    rootDir: string
    peerDependencyIssues: Pick<PeerDependencyIssues, 'bad' | 'missing'>
  }
): PeersResolution {
  const resolvedPeers: { [alias: string]: string } = {}
  const missingPeers = []
  for (const peerName in ctx.resolvedPackage.peerDependencies) { // eslint-disable-line:forin
    const peerVersionRange = ctx.resolvedPackage.peerDependencies[peerName].replace(/^workspace:/, '')

    const resolved = ctx.parentPkgs[peerName]
    const optionalPeer = ctx.resolvedPackage.peerDependenciesMeta?.[peerName]?.optional === true

    if (!resolved) {
      missingPeers.push(peerName)
      const location = getLocationFromNodeIdAndPkg({
        dependenciesTree: ctx.dependenciesTree,
        nodeId: ctx.nodeId,
        pkg: ctx.resolvedPackage,
      })
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
      const location = getLocationFromNodeIdAndPkg({
        dependenciesTree: ctx.dependenciesTree,
        nodeId: ctx.nodeId,
        pkg: ctx.resolvedPackage,
      })
      if (!ctx.peerDependencyIssues.bad[peerName]) {
        ctx.peerDependencyIssues.bad[peerName] = []
      }
      const peerLocation = resolved.nodeId == null
        ? []
        : getLocationFromNodeId({
          dependenciesTree: ctx.dependenciesTree,
          nodeId: resolved.nodeId,
        }).parents
      ctx.peerDependencyIssues.bad[peerName].push({
        foundVersion: resolved.version,
        resolvedFrom: peerLocation,
        parents: location.parents,
        optional: optionalPeer,
        wantedRange: peerVersionRange,
      })
    }

    if (resolved?.nodeId) resolvedPeers[peerName] = resolved.nodeId
  }
  return { resolvedPeers, missingPeers }
}

function getLocationFromNodeIdAndPkg<T> (
  {
    dependenciesTree,
    nodeId,
    pkg,
  }: {
    dependenciesTree: DependenciesTree<T>
    nodeId: string
    pkg: { name: string, version: string }
  }
) {
  const { projectId, parents } = getLocationFromNodeId({ dependenciesTree, nodeId })
  parents.push({ name: pkg.name, version: pkg.version })
  return {
    projectId,
    parents,
  }
}

function getLocationFromNodeId<T> (
  {
    dependenciesTree,
    nodeId,
  }: {
    dependenciesTree: DependenciesTree<T>
    nodeId: string
  }
) {
  const parts = splitNodeId(nodeId).slice(0, -1)
  const parents = scan((prevNodeId, pkgId) => createNodeId(prevNodeId, pkgId), '>', parts)
    .slice(2)
    .map((nid) => pick(['name', 'version'], dependenciesTree[nid].resolvedPackage as ResolvedPackage))
  return {
    projectId: parts[0],
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
  nodeId?: string
  alias?: string
}

function toPkgByName<T extends PartialResolvedPackage> (nodes: Array<{ alias: string, nodeId: string, node: DependenciesTreeNode<T> }>): ParentRefs {
  const pkgsByName: ParentRefs = {}
  const _updateParentRefs = updateParentRefs.bind(null, pkgsByName)
  for (const { alias, node, nodeId } of nodes) {
    const pkg = {
      alias,
      depth: node.depth,
      nodeId,
      version: node.resolvedPackage.version,
    }
    _updateParentRefs(alias, pkg)
    if (alias !== node.resolvedPackage.name) {
      _updateParentRefs(node.resolvedPackage.name, pkg)
    }
  }
  return pkgsByName
}

function updateParentRefs (parentRefs: ParentRefs, newAlias: string, pkg: ParentRef) {
  const existing = parentRefs[newAlias]
  if (existing) {
    const existingHasAlias = existing.alias != null && existing.alias !== newAlias
    if (!existingHasAlias) return
    const newHasAlias = pkg.alias != null && pkg.alias !== newAlias
    if (newHasAlias && semver.gte(existing.version, pkg.version)) return
  }
  parentRefs[newAlias] = pkg
}
