import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { getTreeNodeChildId } from './getTreeNodeChildId.js'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId.js'

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
