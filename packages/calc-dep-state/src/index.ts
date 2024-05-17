import { ENGINE_NAME } from '@pnpm/constants'
import { refToRelative } from '@pnpm/dependency-path'
import { type Lockfile } from '@pnpm/lockfile-types'
import { packageIdFromSnapshot } from '@pnpm/lockfile-utils'
import { type DepPath, type PkgId } from '@pnpm/types'
import { hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import sortKeys from 'sort-keys'

export type DepsGraph<T extends string> = Record<T, DepsGraphNode<T>>

export interface DepsGraphNode<T extends string> {
  children: { [alias: string]: T }
  packageId: PkgId
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
  parents: Set<PkgId>
): DepStateObj {
  if (cache[depPath]) return cache[depPath]
  const node = depsGraph[depPath]
  if (!node) return {}
  const nextParents = new Set([...Array.from(parents), node.packageId])
  const state: DepStateObj = {}
  for (const childId of Object.values(node.children)) {
    const child = depsGraph[childId]
    if (!child) continue
    if (parents.has(child.packageId)) {
      state[child.packageId] = {}
      continue
    }
    state[child.packageId] = calcDepStateObj(childId, depsGraph, cache, nextParents)
  }
  cache[depPath] = sortKeys(state)
  return cache[depPath]
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
        packageId: packageIdFromSnapshot(depPath as DepPath, pkgSnapshot),
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
