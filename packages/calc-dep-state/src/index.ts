import { ENGINE_NAME } from '@pnpm/constants'
import { refToRelative } from '@pnpm/dependency-path'
import { type Lockfile } from '@pnpm/lockfile-types'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import sortKeys from 'sort-keys'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'

export interface DepsGraph {
  [depPath: string]: DepsGraphNode
}

export interface DepsGraphNode {
  children: { [alias: string]: string }
  depPath: string
}

export interface DepsStateCache {
  [depPath: string]: DepStateObj
}

export interface DepStateObj {
  [depPath: string]: DepStateObj
}

export function calcDepState (
  depsGraph: DepsGraph,
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
    result += `-${JSON.stringify(depStateObj)}`
  }
  if (opts.patchFileHash) {
    result += `-${opts.patchFileHash}`
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

export function lockfileToDepGraphWithHashes (lockfile: Lockfile): DepsGraph {
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
  const newGraph: DepsGraph = {}
  const cache: DepsStateCache = {}
  for (const [depPath, gv] of Object.entries(graph)) {
    const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, lockfile.packages![depPath])
    const h = `${pkgName}@${pkgVersion}_${createBase32Hash(calcDepState(graph, cache, depPath, { isBuilt: true }))}`
    const newChildren: Record<string, string> = {}
    for (const [alias, depPathChild] of Object.entries(gv.children)) {
      const { name: pkgNameC, version: pkgVersionC } = nameVerFromPkgSnapshot(depPathChild, lockfile.packages![depPathChild])
      newChildren[alias] = `${pkgNameC}@${pkgVersionC}_${createBase32Hash(calcDepState(graph, cache, depPathChild, { isBuilt: true }))}`
    }
    newGraph[h] = {
      depPath,
      children: newChildren,
    }
  }
  return newGraph
}

export function lockfileToDepGraph (lockfile: Lockfile): DepsGraph {
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

function lockfileDepsToGraphChildren (deps: Record<string, string>): Record<string, string> {
  const children: Record<string, string> = {}
  for (const [alias, reference] of Object.entries(deps)) {
    const depPath = refToRelative(reference, alias)
    if (depPath) {
      children[alias] = depPath
    }
  }
  return children
}
