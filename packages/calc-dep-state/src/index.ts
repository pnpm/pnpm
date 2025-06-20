import { ENGINE_NAME } from '@pnpm/constants'
import { getPkgIdWithPatchHash, refToRelative } from '@pnpm/dependency-path'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'
import { hashObjectWithoutSorting, hashObject } from '@pnpm/crypto.object-hasher'
import { type LockfileResolution, type LockfileObject } from '@pnpm/lockfile.types'

export type DepsGraph<T extends string> = Record<T, DepsGraphNode<T>>

export interface DepsGraphNode<T extends string> {
  children: { [alias: string]: T }
  pkgIdWithPatchHash?: PkgIdWithPatchHash
  resolution?: LockfileResolution
  // The full package ID is a unique fingerprint based on the package’s
  // integrity checksum, patch information, and other resolution data.
  fullPkgId?: string
}

export interface DepsStateCache {
  [depPath: string]: string
}

export function calcDepState<T extends string> (
  depsGraph: DepsGraph<T>,
  cache: DepsStateCache,
  depPath: string,
  opts: {
    patchFileHash?: string
    includeDepGraphHash: boolean
  }
): string {
  let result = ENGINE_NAME
  if (opts.includeDepGraphHash) {
    const depGraphHash = calcDepGraphHash(depsGraph, cache, new Set(), depPath)
    result += `;deps=${depGraphHash}`
  }
  if (opts.patchFileHash) {
    result += `;patch=${opts.patchFileHash}`
  }
  return result
}

function calcDepGraphHash<T extends string> (
  depsGraph: DepsGraph<T>,
  cache: DepsStateCache,
  parents: Set<string>,
  depPath: T
): string {
  if (cache[depPath]) return cache[depPath]
  const node = depsGraph[depPath]
  if (!node) return ''
  node.fullPkgId ??= createFullPkgId(node.pkgIdWithPatchHash!, node.resolution!)
  const deps: Record<string, string> = {}
  if (Object.keys(node.children).length && !parents.has(node.fullPkgId)) {
    const nextParents = new Set([...Array.from(parents), node.fullPkgId])
    const _calcDepGraphHash = calcDepGraphHash.bind(null, depsGraph, cache, nextParents)
    for (const alias in node.children) {
      if (Object.hasOwn(node.children, alias)) {
        const childId = node.children[alias]
        deps[alias] = _calcDepGraphHash(childId)
      }
    }
  }
  cache[depPath] = hashObject({
    id: node.fullPkgId,
    deps,
  })
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
  const _calcDepGraphHash = calcDepGraphHash.bind(null, graph, {})
  for (const pkgMeta of pkgMetaIterator) {
    const { name, version, depPath } = pkgMeta
    const state = {
      // Unfortunately, we need to include the engine name in the hash,
      // even though it's only required for packages that are built,
      // or have dependencies that are built.
      // We can't know for sure whether a package needs to be built
      // before it's fetched from the registry.
      // However, we fetch and write packages to node_modules in random order for performance,
      // so we can't determine at this stage which dependencies will be built.
      engine: ENGINE_NAME,
      deps: _calcDepGraphHash(new Set(), depPath),
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
        fullPkgId: createFullPkgId(getPkgIdWithPatchHash(depPath as DepPath), pkgSnapshot.resolution),
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

function createFullPkgId (pkgIdWithPatchHash: PkgIdWithPatchHash, resolution: LockfileResolution): string {
  const res = 'integrity' in resolution ? resolution.integrity : JSON.stringify(resolution)
  return `${pkgIdWithPatchHash}:${res}`
}
