import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import { getPkgIdWithPatchHash, refToRelative } from '@pnpm/deps.path'
import { engineName } from '@pnpm/engine.runtime.system-version'
import type { LockfileObject, LockfileResolution, PackageSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { resolvePlatformSelector, selectPlatformVariant } from '@pnpm/resolving.resolver-base'
import type { AllowBuild, DepPath, PkgIdWithPatchHash, SupportedArchitectures } from '@pnpm/types'
import { familySync } from 'detect-libc'

/**
 * Strip the `node@runtime:` prefix and any peer-context suffix `(...)`
 * from a single snapshot key, returning the bare Node version (e.g.
 * `"22.11.0"`) — or `undefined` if the key isn't a Node runtime pin.
 *
 * Peer-suffixed (`node@runtime:22.11.0(node@22.11.0)`) and bare
 * (`node@runtime:22.11.0`) forms must reduce to the same answer; the
 * pacquet side relies on the same rule for GVS-hash parity.
 */
function extractRuntimeNodeVersion (snapshotKey: string): string | undefined {
  const prefix = 'node@runtime:'
  if (!snapshotKey.startsWith(prefix)) return undefined
  const versionWithPeers = snapshotKey.slice(prefix.length)
  const parenAt = versionWithPeers.indexOf('(')
  return parenAt === -1 ? versionWithPeers : versionWithPeers.slice(0, parenAt)
}

/**
 * Scan an iterable of lockfile snapshot keys for the resolved
 * `engines.runtime` / `devEngines.runtime` Node version and return
 * its bare version string (e.g. `"22.11.0"`), or `undefined` when
 * no snapshot pins a runtime.
 *
 * Pnpm's runtime resolver writes the pinned Node into the lockfile as
 * a snapshot with key `node@runtime:<version>[(<peers>)]`
 * (see [`engine/runtime/node-resolver/src/index.ts`](https://github.com/pnpm/pnpm/blob/29a42efc3b/engine/runtime/node-resolver/src/index.ts)).
 * The first such key found is treated as authoritative. This is fine
 * as an install-wide fallback (project-pin in the typical case), but
 * snapshots that pin their own Node still need
 * {@link readSnapshotRuntimePin} to get a per-snapshot result.
 *
 * Callers typically pass `Object.keys(lockfile.packages ?? {})` — the
 * in-memory `LockfileObject` merges the on-disk `packages:` and
 * `snapshots:` sections under a single `packages` field, so its keys
 * include every snapshot key the install will hash.
 */
export function findRuntimeNodeVersion (snapshotKeys: Iterable<string>): string | undefined {
  for (const key of snapshotKeys) {
    const version = extractRuntimeNodeVersion(key)
    if (version != null) return version
  }
  return undefined
}

/**
 * Read a single graph node's own `engines.runtime` Node pin from its
 * `children` map. The resolver desugars `engines.runtime` declared on
 * a dependency's manifest into `dependencies.node: 'runtime:<version>'`
 * (see [`installing/deps-resolver/src/resolveDependencies.ts`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-resolver/src/resolveDependencies.ts)),
 * which then becomes a `children.node` entry pointing at the
 * `node@runtime:<version>[(peers)]` snapshot key.
 *
 * Returns the bare version (e.g. `"22.11.0"`) when this snapshot pins
 * its own Node — or `undefined` when it doesn't and the caller should
 * fall back to the install-wide pin / host probe.
 *
 * Per-snapshot resolution matters because the bin linker routes
 * lifecycle-script spawns for a pinning package through *that
 * package's* downloaded Node — anchoring the snapshot's GVS engine
 * hash to an install-wide value would produce the wrong
 * side-effects-cache key for cross-pinning installs.
 */
export function readSnapshotRuntimePin (
  children: Record<string, string> | undefined
): string | undefined {
  const ref = children?.node
  return ref != null ? extractRuntimeNodeVersion(ref) : undefined
}

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
    supportedArchitectures?: SupportedArchitectures
    /**
     * Install-wide fallback `engines.runtime` / `devEngines.runtime`
     * Node version (e.g. `"22.11.0"`). Used only when the snapshot at
     * `depPath` doesn't itself pin a Node: per-snapshot pins take
     * precedence so the side-effects-cache key reflects the actual
     * script-runner Node the bin linker would spawn for the package
     * (see {@link readSnapshotRuntimePin}). Typically computed once
     * per install via {@link findRuntimeNodeVersion} over the
     * lockfile's snapshot keys.
     */
    nodeVersion?: string
  }
): string {
  const ownPin = readSnapshotRuntimePin(depsGraph[depPath as T]?.children)
  let result = engineName(ownPin ?? opts.nodeVersion)
  if (opts.includeDepGraphHash) {
    const depGraphHash = calcDepGraphHash(depsGraph, cache, new Set(), depPath, opts.supportedArchitectures)
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
  depPath: T,
  supportedArchitectures?: SupportedArchitectures
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
    node.fullPkgId = createFullPkgId(node.pkgIdWithPatchHash, node.resolution, supportedArchitectures)
  }
  const deps: Record<string, string> = {}
  if (Object.keys(node.children).length && !parents.has(node.fullPkgId)) {
    const nextParents = new Set([...Array.from(parents), node.fullPkgId])
    for (const alias in node.children) {
      if (Object.hasOwn(node.children, alias)) {
        const childId = node.children[alias]
        deps[alias] = calcDepGraphHash(depsGraph, cache, nextParents, childId, supportedArchitectures)
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
  allowBuild?: AllowBuild,
  supportedArchitectures?: SupportedArchitectures,
  /**
   * Install-wide fallback `engines.runtime` / `devEngines.runtime`
   * Node version. Used only for snapshots that don't pin their own
   * Node; pinning snapshots get resolved per-snapshot via
   * {@link readSnapshotRuntimePin} so the GVS engine hash matches
   * the Node the bin linker would actually spawn for each package
   * (see [`bins/linker/src/index.ts`](https://github.com/pnpm/pnpm/blob/29a42efc3b/bins/linker/src/index.ts)).
   * Typically obtained via {@link findRuntimeNodeVersion} over the
   * lockfile's snapshot keys. `undefined` falls back to
   * {@link engineName}'s default (system `node --version`, with
   * `process.version` as a last resort).
   */
  nodeVersion?: string
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
  const ctx = {
    graph,
    cache: {},
    builtDepPaths,
    buildRequiredCache: builtDepPaths !== undefined ? {} : undefined,
    supportedArchitectures,
    nodeVersion,
  }
  for (const pkgMeta of entries) {
    yield {
      hash: calcGraphNodeHash(ctx, pkgMeta),
      pkgMeta,
    }
  }
}

export function calcGraphNodeHash<T extends PkgMeta> (
  { graph, cache, builtDepPaths, buildRequiredCache, supportedArchitectures, nodeVersion }: {
    graph: DepsGraph<DepPath>
    cache: DepsStateCache
    builtDepPaths?: Set<DepPath>
    buildRequiredCache?: Record<string, boolean>
    supportedArchitectures?: SupportedArchitectures
    /** See [`iterateHashedGraphNodes`]'s `nodeVersion` parameter. */
    nodeVersion?: string
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
  // A snapshot that declares `engines.runtime` carries the desugared
  // `node@runtime:<version>` pin as a child; that's the Node the bin
  // linker spawns for its lifecycle scripts, so it has to drive the
  // engine portion of the hash too. Non-pinning siblings fall through
  // to the install-wide value.
  const ownPin = readSnapshotRuntimePin(graph[depPath]?.children)
  const engine = includeEngine ? engineName(ownPin ?? nodeVersion) : null
  const deps = calcDepGraphHash(graph, cache, new Set(), depPath, supportedArchitectures)
  const hexDigest = hashObjectWithoutSorting({ engine, deps }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, version, hexDigest)
}

export function calcLeafGlobalVirtualStorePath (fullPkgId: string, name: string, version: string): string {
  const depsHash = hashObject({ id: fullPkgId, deps: {} })
  const hexDigest = hashObjectWithoutSorting({ engine: null, deps: depsHash }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, version, hexDigest)
}

/**
 * `subdepIds` maps each direct child's alias to its full pkg id
 * (`${name}@${version}:${integrity}`). Each child contributes a leaf hash
 * (no transitive walk) to the parent's hash, so the resulting path differs
 * whenever the set or versions of children change. One level deep only —
 * use {@link calcGraphNodeHash} when full graph traversal is needed.
 */
export function calcGlobalVirtualStorePathWithSubdeps (
  fullPkgId: string,
  name: string,
  version: string,
  subdepIds: Record<string, string>
): string {
  const childHashes: Record<string, string> = {}
  for (const [alias, childFullPkgId] of Object.entries(subdepIds)) {
    childHashes[alias] = hashObject({ id: childFullPkgId, deps: {} })
  }
  const depsHash = hashObject({ id: fullPkgId, deps: childHashes })
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

export function lockfileToDepGraph (
  lockfile: LockfileObject,
  supportedArchitectures?: SupportedArchitectures
): DepsGraph<DepPath> {
  const graph: DepsGraph<DepPath> = {}
  if (lockfile.packages != null) {
    for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages)) {
      const children = lockfileDepsToGraphChildren({
        ...pkgSnapshot.dependencies,
        ...pkgSnapshot.optionalDependencies,
      })
      graph[depPath as DepPath] = {
        children,
        fullPkgId: createFullPkgId(getPkgIdWithPatchHash(depPath as DepPath), pkgSnapshot.resolution, supportedArchitectures),
      }
    }
  }
  return graph
}

function computeBuiltDepPaths (
  entries: Iterable<PkgMeta>,
  allowBuild: AllowBuild
): Set<DepPath> {
  const builtDepPaths = new Set<DepPath>()
  for (const entry of entries) {
    if (allowBuild(entry.depPath) === true) {
      builtDepPaths.add(entry.depPath)
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

function createFullPkgId (
  pkgIdWithPatchHash: PkgIdWithPatchHash,
  resolution: LockfileResolution,
  supportedArchitectures?: SupportedArchitectures
): string {
  if ('integrity' in resolution && resolution.integrity != null) {
    return `${pkgIdWithPatchHash}:${resolution.integrity}`
  }
  if ('type' in resolution && resolution.type === 'variations') {
    // Variations resolutions list every platform variant for a runtime (e.g. all
    // OS/arch combinations for a Node.js version). Hashing the whole object
    // would be identical across hosts, so two projects that install different
    // variants of the same runtime would collide on the same virtual store
    // directory — the first install would "win" and subsequent installs with
    // different --os/--cpu/--libc would silently reuse the cached variant.
    // Incorporate the chosen variant's integrity instead so each variant gets
    // its own entry in the global virtual store.
    const selector = resolvePlatformSelector(supportedArchitectures, {
      platform: process.platform,
      arch: process.arch,
      libc: familySync(),
    })
    const variant = selectPlatformVariant(resolution.variants, selector)
    const chosenResolution = variant?.resolution
    if (chosenResolution && 'integrity' in chosenResolution && chosenResolution.integrity != null) {
      return `${pkgIdWithPatchHash}:${chosenResolution.integrity}`
    }
  }
  return `${pkgIdWithPatchHash}:${hashObject(resolution)}`
}
