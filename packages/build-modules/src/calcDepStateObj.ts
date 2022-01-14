import sortKeys from 'sort-keys'
import { DependenciesGraph, DependenciesGraphNode } from '.'

export interface DepStateObj {
  [depPath: string]: DepStateObj | {}
}

export default function calcDepStateObj (
  node: DependenciesGraphNode | null,
  depsGraph: DependenciesGraph,
  cache: DepStateObj
): DepStateObj {
  return _calcDepStateObj(node, depsGraph, cache, new Set())
}

function _calcDepStateObj (
  node: DependenciesGraphNode | null,
  depsGraph: DependenciesGraph,
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
      cache[child.depPath] = _calcDepStateObj(child, depsGraph, cache, nextParents)
    }
    state[child.depPath] = cache[child.depPath]
  }
  return sortKeys(state)
}
