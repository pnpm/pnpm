import path from 'path'
import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { type Finder, type Registries } from '@pnpm/types'
import { type DependencyGraph } from './buildDependencyGraph.js'
import { type PackageNode } from './PackageNode.js'
import { getPkgInfo } from './getPkgInfo.js'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId.js'

interface GetTreeOpts {
  maxDepth: number
  rewriteLinkVersionDir: string
  includeOptionalDependencies: boolean
  excludePeerDependencies?: boolean
  lockfileDir: string
  onlyProjects?: boolean
  search?: Finder
  skipped: Set<string>
  registries: Registries
  importers: Record<string, ProjectSnapshot>
  depTypes: DepTypes
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  parentDir?: string

  graph: DependencyGraph
  materializationCache: MaterializationCache
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
  children: PackageNode[]
  /** Total number of PackageNode objects in the subtree (recursive). */
  count: number
}

export interface MaterializationCache {
  results: Map<string, CachedSubtree>
  /**
   * Tracks cache keys whose results have already been returned to at
   * least one parent.  On subsequent cache hits the subtree is elided
   * (an empty array is returned) so that the same dependency is not
   * serialized repeatedly — bounding the output tree to O(N) nodes.
   */
  expanded: Set<string>
}

export function getTree (
  opts: GetTreeOpts,
  parentId: TreeNodeId
): PackageNode[] {
  const ancestors = new Set<string>()
  ancestors.add(serializeTreeNodeId(parentId))

  const ctx: MaterializationContext = {
    ...opts,
    ancestors,
  }

  const tree = materializeChildren(ctx, parentId, opts.maxDepth, opts.parentDir)

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
  return fixCircularRefs(tree, circularAncestors)
}

// ---------------------------------------------------------------------------
// Materialize PackageNode[] tree from the graph
// ---------------------------------------------------------------------------

function materializeCacheKey (nodeId: string, depth: number): string {
  if (depth === Infinity) return nodeId
  return `${nodeId}@d${depth}`
}

function countNodes (nodes: PackageNode[]): number {
  let count = 0
  for (const node of nodes) {
    count += 1
    if (node.dependencies?.length) {
      count += countNodes(node.dependencies)
    }
  }
  return count
}

/**
 * Core materialization function.  Walks the pre-built dependency graph to
 * produce the `PackageNode[]` tree that downstream renderers expect.
 *
 * The cache is keyed by `(nodeId, remainingDepth)` and stores the
 * `PackageNode[]` children of a given node.  It is populated
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
): PackageNode[] {
  if (maxDepth <= 0) return []

  const parentSerialized = serializeTreeNodeId(parentId)
  const graphNode = ctx.graph.nodes.get(parentSerialized)
  if (!graphNode) return []

  const childTreeMaxDepth = maxDepth - 1

  const linkedPathBaseDir = parentId.type === 'importer'
    ? path.join(ctx.lockfileDir, parentId.importerId)
    : ctx.lockfileDir

  const resultDependencies: PackageNode[] = []

  for (const edge of graphNode.edges) {
    const { pkgInfo: packageInfo, readManifest } = getPkgInfo({
      alias: edge.alias,
      currentPackages: ctx.currentPackages,
      depTypes: ctx.depTypes,
      rewriteLinkVersionDir: ctx.rewriteLinkVersionDir,
      linkedPathBaseDir,
      peers: graphNode.peers,
      ref: edge.ref,
      registries: ctx.registries,
      skipped: ctx.skipped,
      wantedPackages: ctx.wantedPackages,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
      modulesDir: ctx.modulesDir,
      parentDir,
    })

    const matchedSearched = ctx.search?.({
      alias: edge.alias,
      name: packageInfo.name,
      version: packageInfo.version,
      readManifest,
    })

    let newEntry: PackageNode | null = null

    if (ctx.onlyProjects && edge.targetNodeId?.type !== 'importer') {
      continue
    } else if (edge.targetNodeId == null) {
      // External link or unresolvable — no traversal possible
      if (ctx.search == null || matchedSearched) {
        newEntry = packageInfo
      }
    } else {
      let dependencies: PackageNode[]
      let dedupedCount: number | undefined
      const circular = ctx.ancestors.has(edge.targetId!)

      if (circular) {
        dependencies = []
      } else {
        const cacheKey = materializeCacheKey(edge.targetId!, childTreeMaxDepth)
        const cached = ctx.materializationCache.results.get(cacheKey)

        if (cached !== undefined) {
          // If this subtree was already returned to a parent elsewhere in
          // the output tree, elide it to avoid repeating the same nodes —
          // this bounds the total output to O(N) nodes.
          if (ctx.materializationCache.expanded.has(cacheKey)) {
            dependencies = []
            if (cached.count > 0) {
              dedupedCount = cached.count
            }
          } else {
            ctx.materializationCache.expanded.add(cacheKey)
            dependencies = cached.children
          }
        } else {
          ctx.ancestors.add(edge.targetId!)
          dependencies = materializeChildren(ctx, edge.targetNodeId, childTreeMaxDepth, packageInfo.path)
          ctx.ancestors.delete(edge.targetId!)

          // Always cache — even results with circular truncations.
          ctx.materializationCache.results.set(cacheKey, { children: dependencies, count: countNodes(dependencies) })
          ctx.materializationCache.expanded.add(cacheKey)
        }
      }

      if (dependencies.length > 0) {
        newEntry = {
          ...packageInfo,
          dependencies,
        }
      } else if (ctx.search == null || matchedSearched) {
        newEntry = packageInfo
      }

      if (newEntry != null && dedupedCount != null) {
        newEntry.deduped = true
        newEntry.dedupedDependenciesCount = dedupedCount
      }
    }

    if (newEntry != null) {
      if (matchedSearched) {
        newEntry.searched = true
        if (typeof matchedSearched === 'string') {
          newEntry.searchMessage = matchedSearched
        }
      }
      if (!newEntry.isPeer || !ctx.excludePeerDependencies || newEntry.dependencies?.length) {
        resultDependencies.push(newEntry)
      }
    }
  }

  return resultDependencies
}

// ---------------------------------------------------------------------------
// Phase 3: Fix circular refs missed by cache reuse
// ---------------------------------------------------------------------------

/**
 * Walks the materialized PackageNode[] tree and marks circular back-edges.
 * A node whose `path` matches an ancestor is a cycle — it gets
 * `circular: true` and its dependencies (if any) are stripped.
 *
 * With deduplication in place (deduped nodes are leaves), the walk is O(N).
 */
function fixCircularRefs (
  nodes: PackageNode[],
  ancestors: Set<string>
): PackageNode[] {
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
