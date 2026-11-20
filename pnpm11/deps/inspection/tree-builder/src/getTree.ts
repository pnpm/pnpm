import path from 'node:path'

import type { DepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.fs'
import type { StoreIndex } from '@pnpm/store.index'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { Finder, RegistriesByScope } from '@pnpm/types'

import { type DependencyEdge, type DependencyGraph, isProjectEdge } from './buildDependencyGraph.js'
import type { DependencyNode } from './DependencyNode.js'
import { getPkgInfo } from './getPkgInfo.js'
import { peersSuffixHashFromDepPath } from './peersSuffixHash.js'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId.js'

export interface BaseTreeOpts {
  include: {
    dependencies?: boolean
    devDependencies?: boolean
    optionalDependencies?: boolean
  }
  excludePeerDependencies?: boolean
  lockfileDir: string
  onlyProjects?: boolean
  search?: Finder
  skipped: Set<string>
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  depTypes: DepTypes
  storeDir?: string
  storeIndex?: StoreIndex
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  showDedupedSearchMatches?: boolean
  graph: DependencyGraph
  materializationCache: MaterializationCache
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  hoistedLocations?: Record<string, string[]>
}

interface GetTreeOpts extends BaseTreeOpts {
  maxDepth: number
  rewriteLinkVersionDir: string
  importers: Record<string, ProjectSnapshot>
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
  parentDir?: string
}

// Context object for materializeChildren — holds everything that stays the
// same across recursive calls.
type MaterializationContext =
  Omit<GetTreeOpts, 'maxDepth' | 'parentDir'> & {
    ancestors: Set<string>
  }

// ---------------------------------------------------------------------------
// Materialization cache types
// ---------------------------------------------------------------------------

interface CachedSubtree {
  /** Total number of DependencyNode objects in the subtree (recursive). */
  count: number
  /** Whether any node in this subtree matched the search. */
  hasSearchMatch: boolean
  /** Search match messages (string-typed matches) found in this subtree. */
  searchMessages: string[]
}

/**
 * Caches already-materialized subtrees.  When a subtree is encountered a
 * second time (cache hit), an empty array is returned and the node is marked
 * as deduped — bounding the total output to O(N) nodes.
 */
export type MaterializationCache = Map<string, CachedSubtree>

export function getTree (
  opts: GetTreeOpts,
  parentId: TreeNodeId
): DependencyNode[] {
  const ancestors = new Set<string>()
  ancestors.add(serializeTreeNodeId(parentId))

  const ctx: MaterializationContext = {
    ...opts,
    ancestors,
  }

  const result = materializeChildren(ctx, parentId, opts.maxDepth, opts.parentDir)

  // Seed the ancestors with parentDir (the filesystem path of parentId) so
  // that back-edges to the root of this subtree are detected — the root
  // itself does not appear as a node in the tree, only its children do.
  const circularAncestors = new Set<string>()
  if (opts.parentDir) {
    circularAncestors.add(opts.parentDir)
  }
  return fixCircularRefs(result.nodes, circularAncestors)
}

// ---------------------------------------------------------------------------
// Materialize DependencyNode[] tree from the graph
// ---------------------------------------------------------------------------

function materializeCacheKey (nodeId: string, depth: number): string {
  if (depth === Infinity) return nodeId
  return `${nodeId}@d${depth}`
}

interface MaterializationResult {
  nodes: DependencyNode[]
  /** Total number of DependencyNode objects in `nodes` (recursive). */
  count: number
  /** Whether any node in this subtree matched the search. */
  hasSearchMatch: boolean
  /** Search match messages (string-typed matches) collected from this subtree. */
  searchMessages: string[]
}

/**
 * Core materialization function.  Walks the pre-built dependency graph to
 * produce the `DependencyNode[]` tree that downstream renderers expect.
 *
 * The cache is keyed by `(nodeId, remainingDepth)` and stores the
 * `DependencyNode[]` children of a given node.  It is populated
 * unconditionally, including results where recursion was truncated at a
 * cycle boundary.  Cycle detection uses a mutable `ancestors` Set to
 * stop recursion but does NOT set the `circular` flag — that is handled
 * by `fixCircularRefs` in a separate pass over the final tree.  This
 * keeps cached subtrees free of context-dependent circular markers.
 */
function materializeChildren (
  ctx: MaterializationContext,
  parentId: TreeNodeId,
  maxDepth: number,
  parentDir?: string
): MaterializationResult {
  if (maxDepth <= 0) return { nodes: [], count: 0, hasSearchMatch: false, searchMessages: [] }

  const parentSerialized = serializeTreeNodeId(parentId)
  const graphNode = ctx.graph.nodes.get(parentSerialized)
  if (!graphNode) {
    throw new Error(`Node ${parentSerialized} not found in the dependency graph`)
  }

  const parent: MaterializationParent = {
    id: parentId,
    dir: parentDir,
    peers: graphNode.peers,
    childTreeMaxDepth: maxDepth - 1,
    linkedPathBaseDir: parentId.type === 'importer'
      ? path.join(ctx.lockfileDir, parentId.importerId)
      : ctx.lockfileDir,
  }
  const result: MaterializationAccumulator = {
    nodes: [],
    count: 0,
    hasSearchMatch: false,
    searchMessages: ctx.showDedupedSearchMatches ? [] as string[] : undefined,
  }

  // Sort edges by alias so that deduplication is deterministic:
  // the alphabetically-first dependency always gets fully expanded.
  const sortedEdges = [...graphNode.edges].sort((a, b) => lexCompare(a.alias, b.alias))

  for (const edge of sortedEdges) {
    if (ctx.onlyProjects && !isProjectEdge(parentId, edge)) {
      continue
    }
    materializeEdge(ctx, { parent, edge, result })
  }

  return {
    count: result.count,
    hasSearchMatch: result.hasSearchMatch,
    nodes: result.nodes,
    searchMessages: result.searchMessages ?? [],
  }
}

interface MaterializationParent {
  id: TreeNodeId
  dir?: string
  peers: Set<string>
  childTreeMaxDepth: number
  linkedPathBaseDir: string
}

interface MaterializationAccumulator {
  nodes: DependencyNode[]
  count: number
  hasSearchMatch: boolean
  searchMessages?: string[]
}

interface EdgeMaterialization {
  parent: MaterializationParent
  edge: DependencyEdge
  result: MaterializationAccumulator
}

/**
 * The children of an edge's target. A subtree that was already materialized
 * elsewhere is elided and only described by its `deduped*` fields.
 */
interface TargetSubtree {
  dependencies: DependencyNode[]
  count: number
  hasSearchMatch: boolean
  searchMessages: string[]
  dedupedCount?: number
  dedupedHasSearchMatch: boolean
  dedupedSearchMessages: string[]
}

function materializeEdge (ctx: MaterializationContext, { parent, edge, result }: EdgeMaterialization): void {
  const { pkgInfo: packageInfo, readManifest } = getPkgInfo({
    ...ctx,
    alias: edge.alias,
    ref: edge.ref,
    peers: parent.peers,
    linkedPathBaseDir: parent.linkedPathBaseDir,
    parentDir: parent.dir,
  })
  // A project linked through its publish directory is listed at its own
  // directory.
  if (ctx.onlyProjects && edge.target?.nodeId.type === 'importer') {
    packageInfo.path = path.join(ctx.lockfileDir, edge.target.nodeId.importerId)
  }

  const searchMatch = ctx.search?.({
    alias: edge.alias,
    name: packageInfo.name,
    version: packageInfo.version,
    readManifest,
  })

  const subtree = edge.target == null
    ? createEmptySubtree()
    : materializeTarget(ctx, { target: edge.target, maxDepth: parent.childTreeMaxDepth, dir: packageInfo.path })
  collectSubtreeSearchMatches(result, subtree)

  const newEntry = edge.target == null
    ? selectUnresolvedEntry(ctx, packageInfo, searchMatch)
    : selectTargetEntry(ctx, { packageInfo, searchMatch, subtree, target: edge.target })
  if (newEntry == null) return

  markSearchMatch(newEntry, { searchMatch, subtree, result })
  if (!newEntry.isPeer || !ctx.excludePeerDependencies || newEntry.dependencies?.length) {
    result.nodes.push(newEntry)
    result.count += 1 + (newEntry.dependencies?.length ? subtree.count : 0)
  }
}

function collectSubtreeSearchMatches (result: MaterializationAccumulator, subtree: TargetSubtree): void {
  if (subtree.hasSearchMatch || subtree.dedupedHasSearchMatch) {
    result.hasSearchMatch = true
  }
  result.searchMessages?.push(...subtree.searchMessages, ...subtree.dedupedSearchMessages)
}

function createEmptySubtree (): TargetSubtree {
  return {
    dependencies: [],
    count: 0,
    hasSearchMatch: false,
    searchMessages: [],
    dedupedHasSearchMatch: false,
    dedupedSearchMessages: [],
  }
}

type EdgeTarget = NonNullable<DependencyEdge['target']>

function materializeTarget (
  ctx: MaterializationContext,
  { target, maxDepth, dir }: { target: EdgeTarget, maxDepth: number, dir: string }
): TargetSubtree {
  if (ctx.ancestors.has(target.id)) return createEmptySubtree()

  const cacheKey = materializeCacheKey(target.id, maxDepth)
  const cached = ctx.materializationCache.get(cacheKey)
  if (cached !== undefined) {
    // This subtree was already returned to a parent elsewhere in
    // the output tree — elide it to avoid repeating the same nodes.
    return {
      ...createEmptySubtree(),
      dedupedCount: cached.count > 0 ? cached.count : undefined,
      dedupedHasSearchMatch: ctx.showDedupedSearchMatches ? cached.hasSearchMatch : false,
      dedupedSearchMessages: ctx.showDedupedSearchMatches ? cached.searchMessages : [],
    }
  }

  ctx.ancestors.add(target.id)
  const childResult = materializeChildren(ctx, target.nodeId, maxDepth, dir)
  ctx.ancestors.delete(target.id)

  ctx.materializationCache.set(cacheKey, {
    count: childResult.count,
    hasSearchMatch: childResult.hasSearchMatch,
    searchMessages: childResult.searchMessages,
  })
  return {
    dependencies: childResult.nodes,
    count: childResult.count,
    hasSearchMatch: childResult.hasSearchMatch,
    searchMessages: childResult.searchMessages,
    dedupedHasSearchMatch: false,
    dedupedSearchMessages: [],
  }
}

type SearchMatch = ReturnType<Finder> | undefined

// External link or unresolvable — no traversal possible. With
// onlyProjects, this is a link to a project outside the lockfile, which
// buildDependenciesTree walks and prunes for the search afterwards.
function selectUnresolvedEntry (
  ctx: MaterializationContext,
  packageInfo: DependencyNode,
  searchMatch: SearchMatch
): DependencyNode | undefined {
  return ctx.search == null || searchMatch || ctx.onlyProjects ? packageInfo : undefined
}

interface TargetEntrySelection {
  packageInfo: DependencyNode
  searchMatch: SearchMatch
  subtree: TargetSubtree
  target: EdgeTarget
}

function selectTargetEntry (
  ctx: MaterializationContext,
  { packageInfo, searchMatch, subtree, target }: TargetEntrySelection
): DependencyNode | undefined {
  let newEntry: DependencyNode
  if (subtree.dependencies.length > 0) {
    newEntry = {
      ...packageInfo,
      dependencies: subtree.dependencies,
    }
  } else if (ctx.search == null || searchMatch || subtree.dedupedHasSearchMatch) {
    newEntry = packageInfo
  } else {
    return undefined
  }

  if (subtree.dedupedCount != null) {
    newEntry.deduped = true
    newEntry.dedupedDependenciesCount = subtree.dedupedCount
  }
  if (target.nodeId.type === 'package') {
    const peerHash = peersSuffixHashFromDepPath(target.nodeId.depPath)
    if (peerHash != null) {
      newEntry.peersSuffixHash = peerHash
    }
  }
  return newEntry
}

function markSearchMatch (
  newEntry: DependencyNode,
  { searchMatch, subtree, result }: { searchMatch: SearchMatch, subtree: TargetSubtree, result: MaterializationAccumulator }
): void {
  if (searchMatch) {
    newEntry.searched = true
    result.hasSearchMatch = true
    if (typeof searchMatch === 'string') {
      newEntry.searchMessage = searchMatch
      result.searchMessages?.push(searchMatch)
    }
    return
  }
  if (subtree.dedupedHasSearchMatch) {
    newEntry.searched = true
    if (subtree.dedupedSearchMessages.length > 0) {
      newEntry.searchMessage = subtree.dedupedSearchMessages.join('\n')
    }
  }
}

/**
 * Walks the materialized DependencyNode[] tree and marks circular back-edges.
 * A node whose `path` matches an ancestor is a cycle — it gets
 * `circular: true` and its dependencies (if any) are stripped.
 *
 * With deduplication in place (deduped nodes are leaves), the walk is O(N).
 */
function fixCircularRefs (
  nodes: DependencyNode[],
  ancestors: Set<string>
): DependencyNode[] {
  let changed = false
  const result = nodes.map(node => {
    if (node.path && ancestors.has(node.path)) {
      changed = true
      const { dependencies: _, deduped: _d, dedupedDependenciesCount: _c, ...rest } = node
      return { ...rest, circular: true as const }
    }
    if (!node.dependencies?.length) return node

    ancestors.add(node.path)
    const fixedDeps = fixCircularRefs(node.dependencies, ancestors)
    ancestors.delete(node.path)

    if (fixedDeps !== node.dependencies) {
      changed = true
      return { ...node, dependencies: fixedDeps }
    }
    return node
  })
  return changed ? result : nodes
}
