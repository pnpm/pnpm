import '@total-typescript/ts-reset'
import { ENGINE_NAME } from '@pnpm/constants'
import { refToRelative } from '@pnpm/dependency-path'
import { hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import sortKeys from 'sort-keys'
import { DependenciesGraph, DepsStateCache, DepsGraph, DepStateObj, Lockfile } from '@pnpm/types'

export function calcDepState(
  depsGraph: DependenciesGraph,
  cache: DepsStateCache,
  depPath: string,
  opts: {
    patchFileHash?: string
    isBuilt: boolean
  }
): string {
  let result = ENGINE_NAME
  if (opts.isBuilt) {
    const depStateObj = calcDepStateObj(depPath, depsGraph, cache, new Set())
    result += `-${hashObjectWithoutSorting(depStateObj)}`
  }
  if (opts.patchFileHash) {
    result += `-${opts.patchFileHash}`
  }
  return result
}

function calcDepStateObj(
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
    state[child.depPath] = calcDepStateObj(
      childId,
      depsGraph,
      cache,
      nextParents
    )
  }
  cache[depPath] = sortKeys(state)
  return cache[depPath]
}

export function lockfileToDepGraph(lockfile: Lockfile): DepsGraph {
  const graph: DepsGraph = {}
  if (lockfile.packages != null) {
    Object.entries(lockfile.packages).map(async ([depPath, pkgSnapshot]) => {
      const children = lockfileDepsToGraphChildren({
        ...pkgSnapshot.dependencies,
        ...pkgSnapshot.optionalDependencies,
      })
      graph[depPath] = {
        children,
        depPath,
      }
    })
  }
  return graph
}

function lockfileDepsToGraphChildren(
  deps: Record<string, string>
): Record<string, string> {
  const children: Record<string, string> = {}
  for (const [alias, reference] of Object.entries(deps)) {
    const depPath = refToRelative(reference, alias)
    if (depPath) {
      children[alias] = depPath
    }
  }
  return children
}
