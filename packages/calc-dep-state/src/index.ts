import { ENGINE_NAME } from '@pnpm/constants'
import { getPkgIdWithPatchHash, refToRelative } from '@pnpm/dependency-path'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'
import { hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { sortDirectKeys } from '@pnpm/object.key-sorting'

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
    result += `;deps=${hashObjectWithoutSorting(depStateObj)}`
  }
  if (opts.patchFileHash) {
    result += `;patch=${opts.patchFileHash}`
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
  cache[depPath] = sortDirectKeys(state)
  return cache[depPath]
}

export function * iterateHashedGraphNodes (graph: DepsGraph<DepPath>, pkgMetaIterator: IterableIterator<{ pkgName: string, pkgVersion: string, depPath: DepPath, pkgIdWithPatchHash: PkgIdWithPatchHash }>): IterableIterator<{ depPath: DepPath, hash: string, pkgIdWithPatchHash: PkgIdWithPatchHash }> {
  const cache: DepsStateCache = {}
  for (const { pkgName, pkgVersion, depPath, pkgIdWithPatchHash } of pkgMetaIterator) {
    const state = calcDepState(graph, cache, depPath, { isBuilt: true })
    const hexDigest = hashObjectWithoutSorting(state, { encoding: 'hex' })
    const hash = `${pkgName}/${pkgVersion}/${hexDigest}` as DepPath
    yield {
      depPath: depPath as DepPath,
      hash,
      pkgIdWithPatchHash,
    }
  }
}

export function lockfileToDepGraphWithHashes (lockfile: LockfileObject): IterableIterator<{ depPath: DepPath, hash: string, pkgIdWithPatchHash: PkgIdWithPatchHash }> {
  const graph = lockfileToDepGraph(lockfile)
  return iterateHashedGraphNodes(graph, (function * () {
    if (lockfile.packages) {
      for (const [depPath, pkg] of Object.entries(lockfile.packages)) {
        const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkg)
        yield {
          pkgName,
          pkgVersion,
          depPath: depPath as DepPath,
          pkgIdWithPatchHash: graph[depPath as DepPath].pkgIdWithPatchHash,
        }
      }
    }
  })())
}

export function lockfileToDepGraph (lockfile: LockfileObject): DepsGraph<DepPath> {
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
