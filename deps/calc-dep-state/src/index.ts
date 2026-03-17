import { ENGINE_NAME } from '@pnpm/constants'
import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import { getPkgIdWithPatchHash, refToRelative } from '@pnpm/dependency-path'
import type { LockfileObject, LockfileResolution, PackageSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { AllowBuild, DepPath, PkgIdWithPatchHash } from '@pnpm/types'

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
  if (!node.fullPkgId) {
    if (!node.pkgIdWithPatchHash) {
      throw new Error(`pkgIdWithPatchHash is not defined for ${depPath} in depsGraph`)
    }
    if (!node.resolution) {
      throw new Error(`resolution is not defined for ${depPath} in depsGraph`)
    }
    node.fullPkgId = createFullPkgId(node.pkgIdWithPatchHash, node.resolution)
  }
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
  pkgMetaIterator: PkgMetaIterator<T>,
  allowBuild?: AllowBuild
): IterableIterator<HashedDepPath<T>> {
  let builtDepPaths: Set<DepPath> | undefined
  let entries: Iterable<T>
  if (allowBuild != null) {
    const pkgMetaList = Array.from(pkgMetaIterator)
    builtDepPaths = computeBuiltDepPaths(pkgMetaList, allowBuild)
    entries = pkgMetaList
  } else {
    entries = pkgMetaIterator
  }
  const _calcGraphNodeHash = calcGraphNodeHash.bind(null, {
    graph,
    cache: {},
    builtDepPaths,
    buildRequiredCache: builtDepPaths !== undefined ? {} : undefined,
  })
  for (const pkgMeta of entries) {
    yield {
      hash: _calcGraphNodeHash(pkgMeta),
      pkgMeta,
    }
  }
}

export function calcGraphNodeHash<T extends PkgMeta> (
  { graph, cache, builtDepPaths, buildRequiredCache }: {
    graph: DepsGraph<DepPath>
    cache: DepsStateCache
    builtDepPaths?: Set<DepPath>
    buildRequiredCache?: Record<string, boolean>
  },
  pkgMeta: T
): string {
  const { name, version, depPath } = pkgMeta
  // When builtDepPaths is provided (derived from the allowBuilds config),
  // we only include the engine name for packages that are allowed to build
  // or transitively depend on a package that is allowed to build.
  // This makes GVS hashes engine-agnostic for pure-JS packages,
  // so they survive Node.js upgrades and architecture changes.
  const includeEngine = builtDepPaths === undefined ||
    transitivelyRequiresBuild(graph, builtDepPaths, buildRequiredCache ??= {}, depPath, new Set())
  const engine = includeEngine ? ENGINE_NAME : null
  const deps = calcDepGraphHash(graph, cache, new Set(), depPath)
  const hexDigest = hashObjectWithoutSorting({ engine, deps }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, version, hexDigest)
}

export function calcLeafGlobalVirtualStorePath (fullPkgId: string, name: string, version: string): string {
  const depsHash = hashObject({ id: fullPkgId, deps: {} })
  const hexDigest = hashObjectWithoutSorting({ engine: null, deps: depsHash }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, version, hexDigest)
}

// Use @/ prefix for unscoped packages to maintain uniform 4-level directory depth
// Scoped: @scope/pkg/version/hash
// Unscoped: @/pkg/version/hash
function formatGlobalVirtualStorePath (name: string, version: string, hexDigest: string): string {
  const prefix = name.startsWith('@') ? '' : '@/'
  return `${prefix}${name}/${version}/${hexDigest}`
}

export interface PkgMetaAndSnapshot extends PkgMeta {
  pkgSnapshot: PackageSnapshot
  pkgIdWithPatchHash: PkgIdWithPatchHash
}

export function * iteratePkgMeta (lockfile: LockfileObject, graph: DepsGraph<DepPath>): PkgMetaIterator<PkgMetaAndSnapshot> {
  if (lockfile.packages == null) {
    return
  }
  for (const depPath in lockfile.packages) {
    if (!Object.hasOwn(lockfile.packages, depPath)) {
      continue
    }
    const pkgSnapshot = lockfile.packages[depPath as DepPath]
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    yield {
      name,
      version,
      depPath: depPath as DepPath,
      pkgIdWithPatchHash: graph[depPath as DepPath]?.pkgIdWithPatchHash ?? getPkgIdWithPatchHash(depPath as DepPath),
      pkgSnapshot,
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

function computeBuiltDepPaths (
  entries: Iterable<{ depPath: DepPath; name: string; version: string }>,
  allowBuild: AllowBuild
): Set<DepPath> {
  const builtDepPaths = new Set<DepPath>()
  for (const { depPath, name, version } of entries) {
    if (allowBuild(name, version) === true) {
      builtDepPaths.add(depPath)
    }
  }
  return builtDepPaths
}

function transitivelyRequiresBuild<T extends string> (
  graph: DepsGraph<T>,
  builtDepPaths: Set<T>,
  cache: Record<string, boolean>,
  depPath: T,
  parents: Set<T>
): boolean {
  if (depPath in cache) return cache[depPath]
  if (builtDepPaths.has(depPath)) {
    cache[depPath] = true
    return true
  }
  const node = graph[depPath]
  if (!node) {
    cache[depPath] = false
    return false
  }
  if (parents.has(depPath)) {
    return false
  }
  const nextParents = new Set([...parents, depPath])
  for (const childDepPath of Object.values(node.children) as T[]) {
    if (transitivelyRequiresBuild(graph, builtDepPaths, cache, childDepPath, nextParents)) {
      cache[depPath] = true
      return true
    }
  }
  cache[depPath] = false
  return false
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
  const res = 'integrity' in resolution ? String(resolution.integrity) : hashObject(resolution)
  return `${pkgIdWithPatchHash}:${res}`
}
