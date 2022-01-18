import { ENGINE_NAME } from '@pnpm/constants'
import sortKeys from 'sort-keys'

export interface DepsGraph {
  [depPath: string]: DepsGraphNode
}

export interface DepsGraphNode {
  children: {[alias: string]: string}
  depPath: string
}

export interface DepsStateCache {
  [nodeId: string]: DepStateObj
}

export interface DepStateObj {
  [depPath: string]: DepStateObj | {}
}

export function calcDepState (
  nodeId: string,
  depsGraph: DepsGraph,
  cache: DepsStateCache
): string {
  const depStateObj = calcDepStateObj(nodeId, depsGraph, cache, new Set())
  return `${ENGINE_NAME}-${JSON.stringify(depStateObj)}`
}

function calcDepStateObj (
  nodeId: string,
  depsGraph: DepsGraph,
  cache: DepsStateCache,
  parents: Set<string>
): DepStateObj {
  if (cache[nodeId]) return cache[nodeId]
  const node = depsGraph[nodeId]
  if (!node) return {}
  const nextParents = new Set([...Array.from(parents), node.depPath])
  const state: DepStateObj = {}
  for (const childId of Object.values(node.children)) {
    const child = depsGraph[childId]
    if (!child) continue
    if (parents.has(child.depPath)) {
      state[child.depPath] = {}
      continue
    }
    state[child.depPath] = calcDepStateObj(childId, depsGraph, cache, nextParents)
  }
  cache[nodeId] = sortKeys(state)
  return cache[nodeId]
}
