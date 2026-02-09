import path from 'path'
import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { type Finder, type Registries } from '@pnpm/types'
import { type PackageNode } from './PackageNode.js'
import { getPkgInfo } from './getPkgInfo.js'
import { getTreeNodeChildId } from './getTreeNodeChildId.js'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId.js'

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

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

  // Optional shared graph and cache for reuse across calls
  graph?: DependencyGraph
  materializationCache?: MaterializationCache
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

export interface MaterializationCache {
  results: Map<string, PackageNode[]>
  /**
   * Tracks cache keys whose results have already been returned to at
   * least one parent.  On subsequent cache hits the subtree is elided
   * (an empty array is returned) so that the same dependency is not
   * serialized repeatedly — bounding the output tree to O(N) nodes.
   */
  expanded: Set<string>
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function getTree (
  opts: GetTreeOpts,
  parentId: TreeNodeId
): PackageNode[] {
  // Phase 1: Build the flat dependency graph (or reuse a shared one)
  const graph = opts.graph ?? buildDependencyGraph(parentId, opts)

  // Phase 2: Materialize the PackageNode[] tree from the graph
  const cache: MaterializationCache = opts.materializationCache ?? { results: new Map(), expanded: new Set() }
  const ancestors = new Set<string>()
  ancestors.add(serializeTreeNodeId(parentId))

  return materializeChildren(graph, parentId, opts.maxDepth, ancestors, cache, opts)
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
  graph: DependencyGraph,
  parentId: TreeNodeId,
  maxDepth: number,
  ancestors: Set<string>,
  cache: MaterializationCache,
  opts: GetTreeOpts
): PackageNode[] {
  if (maxDepth <= 0) return []

  const parentSerialized = serializeTreeNodeId(parentId)
  const graphNode = graph.nodes.get(parentSerialized)
  if (!graphNode) return []

  const childTreeMaxDepth = maxDepth - 1

  const linkedPathBaseDir = parentId.type === 'importer'
    ? path.join(opts.lockfileDir, parentId.importerId)
    : opts.lockfileDir

  const resultDependencies: PackageNode[] = []

  for (const edge of graphNode.edges) {
    const { pkgInfo: packageInfo, readManifest } = getPkgInfo({
      alias: edge.alias,
      currentPackages: opts.currentPackages,
      depTypes: opts.depTypes,
      rewriteLinkVersionDir: opts.rewriteLinkVersionDir,
      linkedPathBaseDir,
      peers: graphNode.peers,
      ref: edge.ref,
      registries: opts.registries,
      skipped: opts.skipped,
      wantedPackages: opts.wantedPackages,
      virtualStoreDir: opts.virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      modulesDir: opts.modulesDir,
      parentDir: opts.parentDir,
    })

    const matchedSearched = opts.search?.({
      alias: edge.alias,
      name: packageInfo.name,
      version: packageInfo.version,
      readManifest,
    })

    let newEntry: PackageNode | null = null

    if (opts.onlyProjects && edge.targetNodeId?.type !== 'importer') {
      continue
    } else if (edge.targetNodeId == null) {
      // External link or unresolvable — no traversal possible
      if (opts.search == null || matchedSearched) {
        newEntry = packageInfo
      }
    } else {
      let dependencies: PackageNode[]
      const circular = ancestors.has(edge.targetId!)

      if (circular) {
        dependencies = []
      } else {
        const cacheKey = materializeCacheKey(edge.targetId!, childTreeMaxDepth)
        const cached = cache.results.get(cacheKey)

        if (cached !== undefined) {
          // If this subtree was already returned to a parent elsewhere in
          // the output tree, elide it to avoid repeating the same nodes —
          // this bounds the total output to O(N) nodes.
          if (cache.expanded.has(cacheKey)) {
            dependencies = []
          } else {
            cache.expanded.add(cacheKey)
            dependencies = cached
          }
        } else {
          ancestors.add(edge.targetId!)
          dependencies = materializeChildren(
            graph, edge.targetNodeId, childTreeMaxDepth,
            ancestors, cache,
            { ...opts, maxDepth: childTreeMaxDepth, parentDir: packageInfo.path }
          )
          ancestors.delete(edge.targetId!)

          // Always cache — even results with circular truncations.
          cache.results.set(cacheKey, dependencies)
          cache.expanded.add(cacheKey)
        }
      }

      if (dependencies.length > 0) {
        newEntry = {
          ...packageInfo,
          dependencies,
        }
      } else if (opts.search == null || matchedSearched) {
        newEntry = packageInfo
      }

      if (newEntry != null && circular) {
        newEntry.circular = true
      }
    }

    if (newEntry != null) {
      if (matchedSearched) {
        newEntry.searched = true
        if (typeof matchedSearched === 'string') {
          newEntry.searchMessage = matchedSearched
        }
      }
      if (!newEntry.isPeer || !opts.excludePeerDependencies || newEntry.dependencies?.length) {
        resultDependencies.push(newEntry)
      }
    }
  }

  return resultDependencies
}
