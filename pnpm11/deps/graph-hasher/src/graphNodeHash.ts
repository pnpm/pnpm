import { hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import { engineName } from '@pnpm/engine.runtime.system-version'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { AllowBuild, DepPath, SupportedArchitectures } from '@pnpm/types'

import { calcDepGraphHash, createDepGraphHashContext } from './calcDepGraphHash.js'
import { computeBuildRequiredDepPaths } from './computeBuildRequiredDepPaths.js'
import { formatGlobalVirtualStorePath } from './formatGlobalVirtualStorePath.js'
import { readSnapshotRuntimePin } from './readSnapshotRuntimePin.js'
import type { DepsGraph, DepsStateCache, HashedDepPath, PkgMeta, PkgMetaIterator } from './types.js'

export interface GraphNodeHashOptions {
  allowBuild?: AllowBuild
  supportedArchitectures?: SupportedArchitectures
  /**
   * Install-wide fallback engines.runtime / devEngines.runtime
   * Node version. Used only for snapshots that don't pin their own
   * Node; pinning snapshots get resolved per-snapshot via
   * readSnapshotRuntimePin so the GVS engine hash matches
   * the Node the bin linker would actually spawn for each package.
   * `undefined` falls back to engineName's default (system
   * node --version, with process.version as a last resort).
   */
  nodeVersion?: string
  /**
   * Directory the lockfile lives in. Scopes local directory dependencies to
   * their project and resolves link: targets for recursive dependency
   * hashing. Omitting it is only safe for a lockfile known to contain neither.
   */
  lockfileDir?: string
}

export function * iterateHashedGraphNodes<Meta extends PkgMeta> (
  graph: DepsGraph<DepPath>,
  pkgMetaIterator: PkgMetaIterator<Meta>,
  opts: GraphNodeHashOptions = {}
): IterableIterator<HashedDepPath<Meta>> {
  let buildRequiredDepPaths: Set<DepPath> | undefined
  let entries: Iterable<Meta>
  if (opts.allowBuild != null) {
    const pkgMetaList = Array.from(pkgMetaIterator)
    buildRequiredDepPaths = computeBuildRequiredDepPaths(graph, computeBuiltDepPaths(pkgMetaList, opts.allowBuild))
    entries = pkgMetaList
  } else {
    entries = pkgMetaIterator
  }
  const ctx = {
    graph,
    cache: {},
    buildRequiredDepPaths,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: opts.nodeVersion,
    lockfileDir: opts.lockfileDir,
  }
  for (const pkgMeta of entries) {
    yield {
      hash: calcGraphNodeHash(ctx, pkgMeta),
      pkgMeta,
    }
  }
}

export function calcGraphNodeHash<Meta extends PkgMeta> (
  { graph, cache, buildRequiredDepPaths, supportedArchitectures, nodeVersion, lockfileDir }: {
    graph: DepsGraph<DepPath>
    cache: DepsStateCache
    /** See computeBuildRequiredDepPaths. */
    buildRequiredDepPaths?: Set<DepPath>
    supportedArchitectures?: SupportedArchitectures
    /** See GraphNodeHashOptions.nodeVersion. */
    nodeVersion?: string
    /** See GraphNodeHashOptions.lockfileDir. */
    lockfileDir?: string
  },
  pkgMeta: Meta
): string {
  const { name, version, depPath } = pkgMeta
  // When buildRequiredDepPaths is provided (derived from the allowBuilds
  // config), we only include the engine name for packages that are allowed
  // to build or transitively depend on a package that is allowed to build.
  // This makes GVS hashes engine-agnostic for pure-JS packages,
  // so they survive Node.js upgrades and architecture changes.
  const includeEngine = buildRequiredDepPaths === undefined || buildRequiredDepPaths.has(depPath)
  // A snapshot that declares engines.runtime carries the desugared
  // node@runtime:<version> pin as a child; that's the Node the bin
  // linker spawns for its lifecycle scripts, so it has to drive the
  // engine portion of the hash too. Non-pinning siblings fall through
  // to the install-wide value.
  const ownPin = readSnapshotRuntimePin(graph[depPath]?.children)
  const engine = includeEngine ? engineName(ownPin ?? nodeVersion) : null
  const deps = calcDepGraphHash({
    depsGraph: graph,
    cache,
    parents: new Set(),
    depPath,
    context: createDepGraphHashContext(supportedArchitectures),
  })
  const isLocalDirectory = isLocalDirectoryResolution(graph[depPath]?.resolution)
  // Scoping the slot needs the project's identity; the segment only needs to
  // know that the package is a local directory, so a caller that leaves
  // lockfileDir out still gets a well-formed path.
  const project = isLocalDirectory ? lockfileDir : undefined
  const hexDigest = project == null
    ? hashObjectWithoutSorting({ engine, deps }, { encoding: 'hex' })
    : hashObjectWithoutSorting({ engine, deps, project }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, isLocalDirectory ? LOCAL_DIRECTORY_SEGMENT : version, hexDigest)
}

/**
 * Slot segment that stands in for the version of a package resolved from a
 * local directory. pnpm omits the version from a directory snapshot, so the
 * lockfile has none to offer — and the resolver, which does know it from the
 * manifest, must agree with the lockfile or a re-install would relocate the
 * package. The segment is decoration in a store listing; the digest that
 * follows it is what identifies the slot.
 */
const LOCAL_DIRECTORY_SEGMENT = 'directory'

/**
 * Whether the package came from a local directory — a file: directory
 * dependency or an injected workspace package.
 */
function isLocalDirectoryResolution (resolution: LockfileResolution | undefined): boolean {
  return resolution != null && 'type' in resolution && resolution.type === 'directory'
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
