import path from 'path'
import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { type Finder, type Registries } from '@pnpm/types'
import { type PackageNode } from './PackageNode.js'
import { getPkgInfo } from './getPkgInfo.js'
import { getTreeNodeChildId } from './getTreeNodeChildId.js'
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
// Dependency graph types (Phase 1)
// ---------------------------------------------------------------------------

interface DependencyEdge {
  alias: string
  ref: string
  targetId: string | undefined
  targetNodeId: TreeNodeId | undefined
}

interface DependencyGraphNode {
  nodeId: TreeNodeId
  edges: DependencyEdge[]
  peers: Set<string>
}

export interface DependencyGraph {
  nodes: Map<string, DependencyGraphNode>
}

// ---------------------------------------------------------------------------
// Materialization cache types (Phase 2)
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

  // Fix circular references that were missed due to cache reuse.
  // Cached subtrees may contain nodes that are not marked circular but should
  // be because they are ancestors in the current traversal path (the cache was
  // populated from a different path where those nodes were not ancestors).
  return fixCircularRefs(tree, new Set())
}

// ---------------------------------------------------------------------------
// Phase 1: Build a flat dependency graph from the lockfile
// ---------------------------------------------------------------------------

export function buildDependencyGraph (
  rootId: TreeNodeId,
  opts: {
    currentPackages: PackageSnapshots
    importers: Record<string, ProjectSnapshot>
    includeOptionalDependencies: boolean
    lockfileDir: string
  }
): DependencyGraph {
  const graph: DependencyGraph = { nodes: new Map() }
  const queue: TreeNodeId[] = [rootId]
  const visited = new Set<string>()

  while (queue.length > 0) {
    const nodeId = queue.shift()!
    const serialized = serializeTreeNodeId(nodeId)
    if (visited.has(serialized)) continue
    visited.add(serialized)

    const snapshot = getSnapshot(nodeId, opts)
    if (!snapshot) {
      graph.nodes.set(serialized, { nodeId, edges: [], peers: new Set() })
      continue
    }

    const deps = !opts.includeOptionalDependencies
      ? snapshot.dependencies
      : {
        ...snapshot.dependencies,
        ...snapshot.optionalDependencies,
      }

    const peers = new Set(Object.keys(
      nodeId.type === 'package'
        ? (opts.currentPackages[nodeId.depPath]?.peerDependencies ?? {})
        : {}
    ))

    const edges: DependencyEdge[] = []
    if (deps != null) {
      for (const alias in deps) {
        const ref = deps[alias]
        const targetNodeId = getTreeNodeChildId({
          parentId: nodeId,
          dep: { alias, ref },
          lockfileDir: opts.lockfileDir,
          importers: opts.importers,
        })
        const targetId = targetNodeId != null ? serializeTreeNodeId(targetNodeId) : undefined
        edges.push({ alias, ref, targetId, targetNodeId })

        if (targetNodeId && !visited.has(targetId!)) {
          queue.push(targetNodeId)
        }
      }
    }

    graph.nodes.set(serialized, { nodeId, edges, peers })
  }

  return graph
}

function getSnapshot (
  treeNodeId: TreeNodeId,
  opts: {
    importers: Record<string, ProjectSnapshot>
    currentPackages: PackageSnapshots
  }
) {
  switch (treeNodeId.type) {
  case 'importer':
    return opts.importers[treeNodeId.importerId]
  case 'package':
    return opts.currentPackages[treeNodeId.depPath]
  }
}

// ---------------------------------------------------------------------------
// Phase 2: Materialize PackageNode[] tree from the graph
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
 * unconditionally: results that contain circular-dependency truncations
 * are cached as well.  This means a cached subtree may show a node as
 * "circular" even when it is not strictly an ancestor in a later
 * traversal path, but the full subtree of that node will have been
 * expanded elsewhere in the output tree.  Accepting this minor
 * inaccuracy makes the cache effective on highly cyclic graphs and
 * prevents the exponential blowup that caused the original OOM.
 *
 * Cycle detection itself uses a mutable `ancestors` Set that is
 * completely separate from the cache.
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

      if (newEntry != null && circular) {
        newEntry.circular = true
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
 * Walks the materialized PackageNode[] tree and corrects nodes that should be
 * marked `circular` but were not, because their subtree came from a cached
 * result computed under a different ancestor context.
 *
 * With deduplication in place (deduped nodes are leaves), the walk is O(N).
 */
function fixCircularRefs (
  nodes: PackageNode[],
  ancestors: Set<string>
): PackageNode[] {
  let changed = false
  const result = nodes.map(node => {
    // A node whose path matches an ancestor should be a circular back-edge.
    if (node.path && ancestors.has(node.path) && !node.circular) {
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
