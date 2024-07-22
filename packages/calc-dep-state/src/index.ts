import { ENGINE_NAME } from '@pnpm/constants'
import { getPkgIdWithPatchHash, refToRelative } from '@pnpm/dependency-path'
import { type Lockfile } from '@pnpm/lockfile-types'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'
import { hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import sortKeys from 'sort-keys'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'

export type DepsGraph<T extends string> = Record<T, DepsGraphNode<T>>

export interface DepsGraphNode<T extends string> {
  children: { [alias: string]: T }
  pkgIdWithPatchHash: PkgIdWithPatchHash
}

export interface DepsStateCache {
  [depPath: string]: DepStateObj
}

export interface DepStateObj {
  [depPath: string]: DepStateObj
}

export function calcDepState<T extends string> (
  depsGraph: DepsGraph<T>,
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

function calcDepStateObj<T extends string> (
  depPath: T,
  depsGraph: DepsGraph<T>,
  cache: DepsStateCache,
  parents: Set<PkgIdWithPatchHash>
): DepStateObj {
  if (cache[depPath]) return cache[depPath]
  const node = depsGraph[depPath]
  if (!node) return {}
  const nextParents = new Set([...Array.from(parents), node.pkgIdWithPatchHash])
  const state: DepStateObj = {}
  for (const childId of Object.values(node.children)) {
    const child = depsGraph[childId]
    if (!child) continue
    if (parents.has(child.pkgIdWithPatchHash)) {
      state[child.pkgIdWithPatchHash] = {}
      continue
    }
    state[child.pkgIdWithPatchHash] = calcDepStateObj(childId, depsGraph, cache, nextParents)
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

export function lockfileToDepGraph (lockfile: Lockfile): DepsGraph<DepPath> {
  const graph: DepsGraph<DepPath> = {}
  if (lockfile.packages != null) {
    for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages)) {
      const children = lockfileDepsToGraphChildren({
        ...pkgSnapshot.dependencies,
        ...pkgSnapshot.optionalDependencies,
      })
      graph[depPath as DepPath] = {
        children,
        pkgIdWithPatchHash: getPkgIdWithPatchHash(depPath as DepPath),
      }
    }
  }
  return graph
}

function lockfileDepsToGraphChildren (deps: Record<string, string>): Record<string, DepPath> {
  const children: Record<string, DepPath> = {}
  for (const [alias, reference] of Object.entries(deps)) {
    const depPath = refToRelative(reference, alias)
    if (depPath) {
      children[alias] = depPath
    }
  }
  return children
}
