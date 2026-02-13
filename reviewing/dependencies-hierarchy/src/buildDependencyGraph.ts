import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { getTreeNodeChildId } from './getTreeNodeChildId.js'
import { serializeTreeNodeId, type TreeNodeId } from './TreeNodeId.js'

interface DependencyEdge {
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

export function buildDependencyGraph (
  rootIds: TreeNodeId[],
  opts: {
    currentPackages: PackageSnapshots
    importers: Record<string, ProjectSnapshot>
    include: {
      dependencies?: boolean
      devDependencies?: boolean
      optionalDependencies?: boolean
    }
    lockfileDir: string
  }
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

    const snapshot = getSnapshot(nodeId, opts)
    if (!snapshot) {
      graph.nodes.set(serialized, { nodeId, edges: [], peers: new Set() })
      continue
    }

    // For importers, only include the dependency fields the caller selected.
    // For packages, devDependencies don't exist in the lockfile.
    const deps = nodeId.type === 'importer'
      ? {
        ...(opts.include.dependencies !== false ? snapshot.dependencies : undefined),
        ...(opts.include.devDependencies !== false ? (snapshot as ProjectSnapshot).devDependencies : undefined),
        ...(opts.include.optionalDependencies ? snapshot.optionalDependencies : undefined),
      }
      : !opts.include.optionalDependencies
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
        const target = targetNodeId != null
          ? { id: serializeTreeNodeId(targetNodeId), nodeId: targetNodeId }
          : undefined
        edges.push({ alias, ref, target })

        if (target && !visited.has(target.id)) {
          queue.push(target.nodeId)
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
