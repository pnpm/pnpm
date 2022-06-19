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
  [depPath: string]: DepStateObj
}

export interface DepStateObj {
  [depPath: string]: DepStateObj | {}
}

export function calcDepState (
  depsGraph: DepsGraph,
  cache: DepsStateCache,
  depPath: string,
  patchFileHash?: string
): string {
  const depStateObj = calcDepStateObj(depPath, depsGraph, cache, new Set())
  let result = `${ENGINE_NAME}-${JSON.stringify(depStateObj)}`
  if (patchFileHash) {
    result += `-${patchFileHash}`
  }
  return result
}

function calcDepStateObj (
  depPath: string,
  depsGraph: DepsGraph,
  cache: DepsStateCache,
  parents: Set<string>
): DepStateObj {
  if (cache[depPath]) return cache[depPath]
  const node = depsGraph[depPath]
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
  cache[depPath] = sortKeys(state)
  return cache[depPath]
}
