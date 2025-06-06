import { ENGINE_NAME } from '@pnpm/constants'
import { getPkgIdWithPatchHash, refToRelative, createUniquePackageId } from '@pnpm/dependency-path'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'
import { hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import { type LockfileResolution, type LockfileObject } from '@pnpm/lockfile.types'
import { sortDirectKeys } from '@pnpm/object.key-sorting'

export type DepsGraph<T extends string> = Record<T, DepsGraphNode<T>>

export interface DepsGraphNode<T extends string> {
  children: { [alias: string]: T }
  pkgIdWithPatchHash?: PkgIdWithPatchHash
  resolution?: LockfileResolution
  uniquePkgId?: string
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
    includeSubdepsHash: boolean
  }
): string {
  let result = ENGINE_NAME
  if (opts.includeSubdepsHash) {
    const depStateObj = calcDepStateObj(depsGraph, cache, depPath, new Set())
    result += `;deps=${hashObjectWithoutSorting(depStateObj)}`
  }
  if (opts.patchFileHash) {
    result += `;patch=${opts.patchFileHash}`
  }
  return result
}

function calcDepStateObj<T extends string> (
  depsGraph: DepsGraph<T>,
  cache: DepsStateCache,
  depPath: T,
  parents: Set<string>
): DepStateObj {
  if (cache[depPath]) return cache[depPath]
  const node = depsGraph[depPath]
  if (!node) return {}
  node.uniquePkgId ??= createUniquePackageId(node.pkgIdWithPatchHash!, node.resolution!)
  const nextParents = new Set([...Array.from(parents), node.uniquePkgId])
  const state: DepStateObj = {}
  for (const childId of Object.values(node.children)) {
    const child = depsGraph[childId]
    if (!child) continue
    child.uniquePkgId ??= createUniquePackageId(child.pkgIdWithPatchHash!, child.resolution!)
    if (parents.has(child.uniquePkgId)) {
      state[child.uniquePkgId] = {}
      continue
    }
    state[child.uniquePkgId] = calcDepStateObj(depsGraph, cache, childId, nextParents)
  }
  cache[depPath] = sortDirectKeys(state)
  return cache[depPath]
}

export interface PkgMeta {
  depPath: DepPath
  name: string
  version: string
}

export type PkgMetaIterator<T extends PkgMeta> = IterableIterator<T>

export interface HashedDepPath<T extends PkgMeta> {
  pkgMeta: T
  hash: string
}

export function * iterateHashedGraphNodes<T extends PkgMeta> (
  graph: DepsGraph<DepPath>,
  pkgMetaIterator: PkgMetaIterator<T>
): IterableIterator<HashedDepPath<T>> {
  const _calcDepStateObj = calcDepStateObj.bind(null, graph, {})
  for (const pkgMeta of pkgMetaIterator) {
    const { name, version, depPath } = pkgMeta

    const node = graph[depPath]
    node.uniquePkgId ??= createUniquePackageId(node.pkgIdWithPatchHash!, node.resolution!)

    const state = {
      engine: ENGINE_NAME,
      deps: _calcDepStateObj(depPath, new Set()),
      ownId: node.uniquePkgId,
    }
    const hexDigest = hashObjectWithoutSorting(state, { encoding: 'hex' })
    yield {
      hash: `${name}/${version}/${hexDigest}`,
      pkgMeta,
    }
  }
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
        uniquePkgId: createUniquePackageId(getPkgIdWithPatchHash(depPath as DepPath), pkgSnapshot.resolution),
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
