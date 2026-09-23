import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, PackageSnapshots, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { DependenciesField, DepPath, ProjectId } from '@pnpm/types'

/**
 * The aliases, per snapshot, of the entries that only satisfy an optional peer
 * of that snapshot.
 *
 * A snapshot records the package peer resolution picked for each of its peers
 * as an ordinary `dependencies` or `optionalDependencies` entry. For an
 * optional peer, the entry is a peer-satisfaction edge when every importer that
 * reaches the snapshot lists the peer target itself. Each of those importers'
 * own dependency field then decides whether the target is installed, so a walk
 * filtered by dependency type must not follow the entry: a devDependency that
 * satisfies a production package's optional peer is not part of a production
 * install. An entry an importer reaches without listing the target is followed,
 * because for that importer the entry is what provides the target (an ancestor
 * package or another importer supplied it). Required peers are always followed.
 */
export type PeerSatisfactionEdges = ReadonlyMap<DepPath, ReadonlySet<string>>

export interface PeerSatisfactionEdgesOptions {
  /**
   * Count the direct dependencies of the workspace root importer as listed by
   * every importer, as peer resolution does when this setting is on. Defaults
   * to `false`, which follows more edges.
   */
  resolvePeersFromWorkspaceRoot?: boolean
}

export type IncludedDependencyGroups = { [dependenciesField in DependenciesField]?: boolean }

const cache = new WeakMap<LockfileObject, { withRoot?: PeerSatisfactionEdges, withoutRoot?: PeerSatisfactionEdges }>()

export function getPeerSatisfactionEdges (
  lockfile: LockfileObject,
  opts?: PeerSatisfactionEdgesOptions
): PeerSatisfactionEdges {
  let cached = cache.get(lockfile)
  if (cached == null) {
    cached = {}
    cache.set(lockfile, cached)
  }
  const key = opts?.resolvePeersFromWorkspaceRoot === true ? 'withRoot' : 'withoutRoot'
  let edges = cached[key]
  if (edges == null) {
    edges = collectPeerSatisfactionEdges(lockfile, opts?.resolvePeersFromWorkspaceRoot === true)
    cached[key] = edges
  }
  return edges
}

/**
 * The peer-satisfaction edges a walk under `include` skips, or `undefined`
 * when `include` keeps every dependency group, in which case every edge is
 * followed.
 */
export function getPeerSatisfactionEdgesToSkip (
  lockfile: LockfileObject,
  opts: PeerSatisfactionEdgesOptions & { include?: IncludedDependencyGroups }
): PeerSatisfactionEdges | undefined {
  if (!excludesDependencyGroup(opts.include)) return undefined
  const edges = getPeerSatisfactionEdges(lockfile, opts)
  return edges.size === 0 ? undefined : edges
}

export function excludesDependencyGroup (include: IncludedDependencyGroups | undefined): boolean {
  return include != null && (
    include.dependencies === false ||
    include.devDependencies === false ||
    include.optionalDependencies === false
  )
}

export function isPeerSatisfactionEdge (
  edges: PeerSatisfactionEdges | undefined,
  parent: DepPath,
  alias: string
): boolean {
  return edges?.get(parent)?.has(alias) === true
}

/**
 * Removes, from each snapshot of a filtered `packages` map, the
 * peer-satisfaction entries whose target is not in the map, so that the
 * filtered lockfile does not reference a package it does not contain. An entry
 * whose target the map keeps through another path stays: that target is
 * installed, so it is linked. The snapshots are copied, never mutated.
 */
export function omitUnretainedPeerSatisfactionEdges (
  packages: PackageSnapshots,
  edges: PeerSatisfactionEdges | undefined
): PackageSnapshots {
  if (edges == null || edges.size === 0) return packages
  let result: PackageSnapshots | undefined
  for (const [parent, aliases] of edges) {
    const snapshot = packages[parent]
    if (snapshot == null) continue
    const dependencies = omitUnretainedTargets(snapshot.dependencies, aliases, packages)
    const optionalDependencies = omitUnretainedTargets(snapshot.optionalDependencies, aliases, packages)
    if (dependencies === snapshot.dependencies && optionalDependencies === snapshot.optionalDependencies) continue
    const copy: PackageSnapshot = { ...snapshot }
    setOrDelete(copy, 'dependencies', dependencies)
    setOrDelete(copy, 'optionalDependencies', optionalDependencies)
    result ??= { ...packages }
    result[parent] = copy
  }
  return result ?? packages
}

function omitUnretainedTargets (
  deps: ResolvedDependencies | undefined,
  aliases: ReadonlySet<string>,
  packages: PackageSnapshots
): ResolvedDependencies | undefined {
  if (deps == null) return deps
  let result: ResolvedDependencies | undefined
  for (const alias of aliases) {
    if (!Object.hasOwn(deps, alias)) continue
    const target = dp.refToRelative(deps[alias], alias)
    if (target == null || Object.hasOwn(packages, target)) continue
    result ??= { ...deps }
    delete result[alias]
  }
  return result ?? deps
}

function setOrDelete (
  snapshot: PackageSnapshot,
  field: 'dependencies' | 'optionalDependencies',
  deps: ResolvedDependencies | undefined
): void {
  if (deps == null || Object.keys(deps).length === 0) {
    delete snapshot[field]
  } else {
    snapshot[field] = deps
  }
}

interface OptionalPeerEntry {
  parent: DepPath
  alias: string
  target: DepPath
}

function collectPeerSatisfactionEdges (lockfile: LockfileObject, resolvePeersFromWorkspaceRoot: boolean): PeerSatisfactionEdges {
  const edges = new Map<DepPath, Set<string>>()
  const entries = collectOptionalPeerEntries(lockfile)
  if (entries.length === 0) return edges

  const importers = Object.values(lockfile.importers)
  const directDepPathsByImporter = importers.map((importer) => new Set(importerDirectDepPaths(importer)))
  const rootImporter = resolvePeersFromWorkspaceRoot ? lockfile.importers['.' as ProjectId] : undefined
  const rootDirectDepPaths = rootImporter == null ? undefined : new Set(importerDirectDepPaths(rootImporter))
  // The walk depends only on which importers do not list the target, so
  // targets with the same set of non-listing importers share one walk.
  const reachedByNonListing = new Map<string, Set<DepPath>>()
  const reachedByTarget = new Map<DepPath, Set<DepPath>>()
  const reachedWithoutListing = (target: DepPath): Set<DepPath> => {
    let reached = reachedByTarget.get(target)
    if (reached != null) return reached
    const nonListing = rootDirectDepPaths?.has(target)
      ? []
      : directDepPathsByImporter.flatMap((direct, index) => direct.has(target) ? [] : [index])
    const key = nonListing.join(',')
    reached = reachedByNonListing.get(key)
    if (reached == null) {
      reached = new Set()
      walkAllEdges(lockfile, nonListing.flatMap((index) => importerDirectDepPaths(importers[index])), reached)
      reachedByNonListing.set(key, reached)
    }
    reachedByTarget.set(target, reached)
    return reached
  }
  for (const { parent, alias, target } of entries) {
    if (reachedWithoutListing(target).has(parent)) continue
    let aliases = edges.get(parent)
    if (aliases == null) {
      aliases = new Set()
      edges.set(parent, aliases)
    }
    aliases.add(alias)
  }
  return edges
}

function collectOptionalPeerEntries (lockfile: LockfileObject): OptionalPeerEntry[] {
  const entries: OptionalPeerEntry[] = []
  for (const [parent, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    if (snapshot.peerDependencies == null || snapshot.peerDependenciesMeta == null) continue
    for (const deps of [snapshot.dependencies, snapshot.optionalDependencies]) {
      if (deps == null) continue
      for (const [alias, ref] of Object.entries(deps)) {
        if (!isOptionalPeer(snapshot, alias)) continue
        const target = dp.refToRelative(ref, alias)
        if (target != null) entries.push({ parent, alias, target })
      }
    }
  }
  return entries
}

// Own-property checks throughout: a lockfile is untrusted input, and `in` or a
// plain lookup also matches inherited Object.prototype names (`constructor`,
// `toString`, ...), some of which are valid package names.
function isOptionalPeer (snapshot: PackageSnapshot, alias: string): boolean {
  return snapshot.peerDependencies != null &&
    Object.hasOwn(snapshot.peerDependencies, alias) &&
    snapshot.peerDependenciesMeta != null &&
    Object.hasOwn(snapshot.peerDependenciesMeta, alias) &&
    snapshot.peerDependenciesMeta[alias]?.optional === true
}

function importerDirectDepPaths (importer: ProjectSnapshot): DepPath[] {
  return [
    ...resolvedDepsToDepPaths(importer.dependencies),
    ...resolvedDepsToDepPaths(importer.devDependencies),
    ...resolvedDepsToDepPaths(importer.optionalDependencies),
  ]
}

// Explicit stack rather than recursion: a deep dependency chain in an untrusted
// lockfile would otherwise overflow the call stack.
function walkAllEdges (lockfile: LockfileObject, depPaths: DepPath[], seen: Set<DepPath>): void {
  const packages = lockfile.packages ?? {}
  const stack = [...depPaths]
  while (stack.length > 0) {
    const depPath = stack.pop()!
    if (seen.has(depPath)) continue
    seen.add(depPath)
    const snapshot = Object.hasOwn(packages, depPath) ? packages[depPath] : undefined
    if (snapshot == null) continue
    for (const child of resolvedDepsToDepPaths(snapshot.dependencies)) stack.push(child)
    for (const child of resolvedDepsToDepPaths(snapshot.optionalDependencies)) stack.push(child)
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies | undefined): DepPath[] {
  const depPaths: DepPath[] = []
  if (deps == null) return depPaths
  for (const [alias, ref] of Object.entries(deps)) {
    const depPath = dp.refToRelative(ref, alias)
    if (depPath != null) depPaths.push(depPath)
  }
  return depPaths
}
