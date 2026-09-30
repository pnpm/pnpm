import type { DependenciesField, DepPath } from '@pnpm/types'

import type { DependenciesGraph, DependenciesGraphNode, DirectDependenciesByImporterId } from './types.js'

export function graphWalker<NodeId extends string> (
  graph: DependenciesGraph<NodeId>,
  directDepsByImporterId: DirectDependenciesByImporterId<NodeId>,
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<DepPath>
  }
): GraphWalker<NodeId> {
  const startNodeIds = [] as NodeId[]
  const allDirectDeps = [] as Array<{ alias: string, nodeId: NodeId }>

  for (const directDeps of Object.values(directDepsByImporterId)) {
    for (const [alias, nodeId] of directDeps.entries()) {
      const depNode = graph[nodeId]
      if (depNode == null) continue
      startNodeIds.push(nodeId)
      allDirectDeps.push({ alias, nodeId })
    }
  }
  const visited = new Set<NodeId>()
  return {
    directDeps: allDirectDeps,
    step: makeStep({
      includeOptionalDependencies: opts?.include?.optionalDependencies !== false,
      graph,
      visited,
      skipped: opts?.skipped,
    }, startNodeIds),
  }
}

function makeStep<NodeId extends string> (
  ctx: {
    includeOptionalDependencies: boolean
    graph: DependenciesGraph<NodeId>
    visited: Set<NodeId>
    skipped?: Set<DepPath>
  },
  nextNodeIds: NodeId[]
): GraphWalkerStep<NodeId> {
  const result: GraphWalkerStep<NodeId> = {
    dependencies: [],
    links: [],
    missing: [],
  }
  const _next = collectChildNodeIds.bind(null, {
    includeOptionalDependencies: ctx.includeOptionalDependencies,
  })
  for (const nodeId of nextNodeIds) {
    if (ctx.visited.has(nodeId)) continue
    ctx.visited.add(nodeId)
    const node = ctx.graph[nodeId]
    if (node == null) {
      recordNodeWithoutGraphEntry(result, nodeId)
      continue
    }
    if (ctx.skipped?.has(node.depPath)) continue
    result.dependencies.push({
      nodeId,
      next: () => makeStep<NodeId>(ctx, _next(node) as NodeId[]),
      node,
    })
  }
  return result
}

function recordNodeWithoutGraphEntry<NodeId extends string> (step: GraphWalkerStep<NodeId>, nodeId: NodeId): void {
  if (nodeId.startsWith('link:')) {
    step.links.push(nodeId)
    return
  }
  step.missing.push(nodeId)
}

function collectChildNodeIds<NodeId extends string> (opts: { includeOptionalDependencies: boolean }, nextPkg: DependenciesGraphNode<NodeId>): NodeId[] {
  if (opts.includeOptionalDependencies) {
    return Object.values(nextPkg.children)
  } else {
    const nextNodeIds: NodeId[] = []
    for (const [alias, nodeId] of Object.entries(nextPkg.children)) {
      if (!nextPkg.optionalDependencies.has(alias)) {
        nextNodeIds.push(nodeId)
      }
    }
    return nextNodeIds
  }
}

export interface GraphWalker<NodeId extends string> {
  directDeps: Array<{
    alias: string
    nodeId: NodeId
  }>
  step: GraphWalkerStep<NodeId>
}

export interface GraphWalkerStep<NodeId extends string> {
  dependencies: Array<GraphDependency<NodeId>>
  links: string[]
  missing: string[]
}

export interface GraphDependency<NodeId extends string> {
  nodeId: NodeId
  node: DependenciesGraphNode<NodeId>
  next: () => GraphWalkerStep<NodeId>
}
