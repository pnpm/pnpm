import path from 'path'
import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { type Finder, type Registries } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { type DependencyGraph } from './buildDependencyGraph.js'
import { type DependencyNode } from './DependencyNode.js'
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
  registries: Registries
  depTypes: DepTypes
  storeDir?: string
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  showDedupedSearchMatches?: boolean
  graph: DependencyGraph
  materializationCache: MaterializationCache
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

  // Mark circular back-edges.  materializeChildren truncates dependencies
  // at cycle boundaries but does not set the `circular` flag, so that cached
  // subtrees stay context-independent.  fixCircularRefs walks the final tree
  // and adds `circular: true` wherever a node's path matches an ancestor.
  //
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

  const childTreeMaxDepth = maxDepth - 1

  const linkedPathBaseDir = parentId.type === 'importer'
    ? path.join(ctx.lockfileDir, parentId.importerId)
    : ctx.lockfileDir

  const resultDependencies: DependencyNode[] = []
  let resultCount = 0
  let resultHasSearchMatch = false
  const resultSearchMessages = ctx.showDedupedSearchMatches ? [] as string[] : undefined

  // Sort edges by alias so that deduplication is deterministic:
  // the alphabetically-first dependency always gets fully expanded.
  const sortedEdges = [...graphNode.edges].sort((a, b) => lexCompare(a.alias, b.alias))

  for (const edge of sortedEdges) {
    if (ctx.onlyProjects && edge.target?.nodeId.type !== 'importer') {
      continue
    }

    const { pkgInfo: packageInfo, readManifest } = getPkgInfo({
      ...ctx,
      alias: edge.alias,
      ref: edge.ref,
      peers: graphNode.peers,
      linkedPathBaseDir,
      parentDir,
    })

    const searchMatch = ctx.search?.({
      alias: edge.alias,
      name: packageInfo.name,
      version: packageInfo.version,
      readManifest,
    })

    let newEntry: DependencyNode | null = null
    let childCount = 0
    let dedupedHasSearchMatch = false
    let dedupedSearchMessages: string[] = []

    if (edge.target == null) {
      // External link or unresolvable — no traversal possible
      if (ctx.search == null || searchMatch) {
        newEntry = packageInfo
      } else {
        continue
      }
    } else {
      let dependencies: DependencyNode[]
      let childHasSearchMatch = false
      let childSearchMessages: string[] = []
      let dedupedCount: number | undefined
      const circular = ctx.ancestors.has(edge.target.id)

      if (circular) {
        dependencies = []
      } else {
        const cacheKey = materializeCacheKey(edge.target.id, childTreeMaxDepth)
        const cached = ctx.materializationCache.get(cacheKey)

        if (cached !== undefined) {
          // This subtree was already returned to a parent elsewhere in
          // the output tree — elide it to avoid repeating the same nodes.
          dependencies = []
          if (cached.count > 0) {
            dedupedCount = cached.count
          }
          if (ctx.showDedupedSearchMatches) {
            dedupedHasSearchMatch = cached.hasSearchMatch
            dedupedSearchMessages = cached.searchMessages
          }
        } else {
          ctx.ancestors.add(edge.target.id)
          const childResult = materializeChildren(ctx, edge.target.nodeId, childTreeMaxDepth, packageInfo.path)
          ctx.ancestors.delete(edge.target.id)

          dependencies = childResult.nodes
          childCount = childResult.count
          childHasSearchMatch = childResult.hasSearchMatch
          childSearchMessages = childResult.searchMessages

          // Always cache — even results with circular truncations.
          ctx.materializationCache.set(cacheKey, {
            count: childCount,
            hasSearchMatch: childHasSearchMatch,
            searchMessages: childSearchMessages,
          })
        }
        if (childHasSearchMatch || dedupedHasSearchMatch) {
          resultHasSearchMatch = true
        }
        resultSearchMessages?.push(...childSearchMessages, ...dedupedSearchMessages)
      }

      if (dependencies.length > 0) {
        newEntry = {
          ...packageInfo,
          dependencies,
        }
      } else if (ctx.search == null || searchMatch || dedupedHasSearchMatch) {
        newEntry = packageInfo
      } else {
        continue
      }

      if (dedupedCount != null) {
        newEntry.deduped = true
        newEntry.dedupedDependenciesCount = dedupedCount
      }
      if (edge.target.nodeId.type === 'package') {
        const peerHash = peersSuffixHashFromDepPath(edge.target.nodeId.depPath)
        if (peerHash != null) {
          newEntry.peersSuffixHash = peerHash
        }
      }
    }

    if (searchMatch) {
      newEntry.searched = true
      resultHasSearchMatch = true
      if (typeof searchMatch === 'string') {
        newEntry.searchMessage = searchMatch
        resultSearchMessages?.push(searchMatch)
      }
    } else if (dedupedHasSearchMatch) {
      newEntry.searched = true
      if (dedupedSearchMessages.length > 0) {
        newEntry.searchMessage = dedupedSearchMessages.join('\n')
      }
    }
    if (!newEntry.isPeer || !ctx.excludePeerDependencies || newEntry.dependencies?.length) {
      resultDependencies.push(newEntry)
      resultCount += 1 + (newEntry.dependencies?.length ? childCount : 0)
    }
  }

  return {
    count: resultCount,
    hasSearchMatch: resultHasSearchMatch,
    nodes: resultDependencies,
    searchMessages: resultSearchMessages ?? [],
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
    // A node whose path matches an ancestor is a circular back-edge.
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
