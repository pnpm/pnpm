import type { PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { isPeerSatisfactionEdge, type PeerSatisfactionEdges } from '@pnpm/lockfile.peer-edges'

import { getTreeNodeChildId } from './getTreeNodeChildId.js'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId.js'

export interface DependencyEdge {
  alias: string
  ref: string
  target?: {
    id: string
    nodeId: TreeNodeId
  }
}

interface DependencyGraphNode {
  nodeId: TreeNodeId
  edges: DependencyEdge[]
  peers: Set<string>
}

export interface DependencyGraph {
  nodes: Map<string, DependencyGraphNode>
}

interface DependencyGraphOptions {
  currentPackages: PackageSnapshots
  importers: Record<string, ProjectSnapshot>
  include: {
    dependencies?: boolean
    devDependencies?: boolean
    optionalDependencies?: boolean
  }
  lockfileDir: string
  onlyProjects?: boolean
  /** Edges left out of the graph, from `getPeerSatisfactionEdgesToSkip`. */
  peerSatisfactionEdges?: PeerSatisfactionEdges
}

export function buildDependencyGraph (
  rootIds: TreeNodeId[],
  opts: DependencyGraphOptions
): DependencyGraph {
  const graph: DependencyGraph = { nodes: new Map() }
  const queue: TreeNodeId[] = [...rootIds]
  let queueIdx = 0
  const visited = new Set<string>()

  while (queueIdx < queue.length) {
    const nodeId = queue[queueIdx++]
    const serialized = serializeTreeNodeId(nodeId)
    if (visited.has(serialized)) continue
    visited.add(serialized)

    const graphNode = createGraphNode(nodeId, opts)
    for (const { target } of graphNode.edges) {
      if (target && !visited.has(target.id)) {
        queue.push(target.nodeId)
      }
    }
    graph.nodes.set(serialized, graphNode)
  }

  return graph
}

function createGraphNode (nodeId: TreeNodeId, opts: DependencyGraphOptions): DependencyGraphNode {
  const snapshot = getSnapshot(nodeId, opts)
  if (!snapshot) {
    return { nodeId, edges: [], peers: new Set() }
  }

  // For importers, only include the dependency fields the caller selected.
  // For packages, devDependencies don't exist in the lockfile.
  const deps = nodeId.type === 'importer'
    ? selectImporterDependencies(snapshot as ProjectSnapshot, opts.include)
    : selectPackageDependencies(snapshot, opts.include)

  const peers = new Set(Object.keys(
    nodeId.type === 'package'
      ? (opts.currentPackages[nodeId.depPath]?.peerDependencies ?? {})
      : {}
  ))

  return { nodeId, edges: createEdges(nodeId, deps, opts), peers }
}

function selectImporterDependencies (snapshot: ProjectSnapshot, include: DependencyGraphOptions['include']): DependencyRefs {
  return {
    ...(include.dependencies !== false ? snapshot.dependencies : undefined),
    ...(include.devDependencies !== false ? snapshot.devDependencies : undefined),
    ...(include.dependencies !== false && include.optionalDependencies ? snapshot.optionalDependencies : undefined),
  }
}

function selectPackageDependencies (
  snapshot: NonNullable<ReturnType<typeof getSnapshot>>,
  include: DependencyGraphOptions['include']
): DependencyRefs | undefined {
  if (!include.optionalDependencies) return snapshot.dependencies
  return {
    ...snapshot.dependencies,
    ...snapshot.optionalDependencies,
  }
}

type DependencyRefs = Record<string, unknown>

function createEdges (nodeId: TreeNodeId, deps: DependencyRefs | undefined, opts: DependencyGraphOptions): DependencyEdge[] {
  const edges: DependencyEdge[] = []
  if (deps == null) return edges
  for (const alias in deps) {
    const edge = createEdge(nodeId, { alias, rawRef: deps[alias] }, opts)
    if (edge == null) continue
    if (opts.onlyProjects && !isProjectEdge(nodeId, edge)) {
      continue
    }
    edges.push(edge)
  }
  return edges
}

function createEdge (
  nodeId: TreeNodeId,
  { alias, rawRef }: { alias: string, rawRef: unknown },
  opts: DependencyGraphOptions
): DependencyEdge | undefined {
  // Lockfile may expose ref as string (version) or inline { version, specifier }
  const ref = typeof rawRef === 'string'
    ? rawRef
    : (rawRef as { version?: string } | null)?.version
  if (ref == null) return undefined
  if (nodeId.type === 'package' && isPeerSatisfactionEdge(opts.peerSatisfactionEdges, nodeId.depPath, alias)) return undefined
  const targetNodeId = getTreeNodeChildId({
    parentId: nodeId,
    dep: { alias, ref },
    lockfileDir: opts.lockfileDir,
    importers: opts.importers,
  })
  const target = targetNodeId != null
    ? { id: serializeTreeNodeId(targetNodeId), nodeId: targetNodeId }
    : undefined
  return { alias, ref, target }
}

/**
 * Whether an edge may lead to a project. Besides the importers of the lockfile,
 * this includes a project's `link:` dependency on a directory outside the
 * lockfile: with a dedicated lockfile per project, every other workspace
 * project is such a directory. `buildDependenciesTree` keeps those only when
 * the directory is a workspace project.
 */
export function isProjectEdge (parentId: TreeNodeId, edge: DependencyEdge): boolean {
  if (edge.target != null) return edge.target.nodeId.type === 'importer'
  return parentId.type === 'importer' && edge.ref.startsWith('link:')
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
