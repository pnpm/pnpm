import { ENGINE_NAME } from '@pnpm/constants'
import sortKeys from 'sort-keys'

export interface DepsGraph {
  [depPath: string]: DepsGraphNode
}

export interface DepsGraphNode {
  children: {[alias: string]: string}
  depPath: string
}

export interface DepStateObj {
  [depPath: string]: DepStateObj | {}
}

export default function calcDepState (
  node: DepsGraphNode | null,
  depsGraph: DepsGraph,
  cache: DepStateObj
): string {
  const depStateObj = calcDepStateObj(node, depsGraph, cache, new Set())
  return `${ENGINE_NAME}-${JSON.stringify(depStateObj)}`
}

function calcDepStateObj (
  node: DepsGraphNode | null,
  depsGraph: DepsGraph,
  cache: DepStateObj,
  parents: Set<string>
): DepStateObj {
  if (!node) return {}
  const nextParents = new Set([...Array.from(parents), node.depPath])
  const state: DepStateObj = {}
  for (const childKey of Object.values(node.children)) {
    const child = depsGraph[childKey]
    if (!child) continue
    if (parents.has(child.depPath)) {
      state[child.depPath] = {}
      continue
    }
    if (!cache[child.depPath]) {
      cache[child.depPath] = calcDepStateObj(child, depsGraph, cache, nextParents)
    }
    state[child.depPath] = cache[child.depPath]
  }
  return sortKeys(state)
}
