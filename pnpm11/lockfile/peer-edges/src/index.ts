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
   * Count the devDependencies of the workspace root importer as listed by
   * every importer, as peer resolution does when this setting is on. A root
   * production dependency does not count: the root is usually outside a
   * filtered or deployed walk, so it would not provide the peer there.
   * Defaults to `false`, which follows more edges.
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
export function pruneDanglingPeerSatisfactionEdges (
  packages: PackageSnapshots,
  edges: PeerSatisfactionEdges | undefined
): PackageSnapshots {
  if (edges == null || edges.size === 0) return packages
  let result: PackageSnapshots | undefined
  for (const [parent, aliases] of edges) {
    const snapshot = packages[parent]
    if (snapshot == null) continue
    const dependencies = omitDroppedTargets(snapshot.dependencies, aliases, packages)
    const optionalDependencies = omitDroppedTargets(snapshot.optionalDependencies, aliases, packages)
    if (dependencies === snapshot.dependencies && optionalDependencies === snapshot.optionalDependencies) continue
    const copy: PackageSnapshot = { ...snapshot }
    setOrDelete(copy, 'dependencies', dependencies)
    setOrDelete(copy, 'optionalDependencies', optionalDependencies)
    result ??= { ...packages }
    result[parent] = copy
  }
  return result ?? packages
}

function omitDroppedTargets (
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

interface NonListingContext {
  directDepPathsByImporter: Array<Set<DepPath>>
  rootDevDepPaths?: Set<DepPath>
}

function collectPeerSatisfactionEdges (lockfile: LockfileObject, resolvePeersFromWorkspaceRoot: boolean): PeerSatisfactionEdges {
  const edges = new Map<DepPath, Set<string>>()
  const entries = collectOptionalPeerEntries(lockfile)
  if (entries.length === 0) return edges

  const importers = Object.values(lockfile.importers)
  const context: NonListingContext = {
    directDepPathsByImporter: importers.map((importer) => new Set(importerDirectDepPaths(importer))),
    rootDevDepPaths: getRootDevDepPaths(lockfile, resolvePeersFromWorkspaceRoot),
  }
  const groupedEntries = groupEntriesByNonListing(entries, context)
  for (const { nonListing, entries: groupEntries } of groupedEntries) {
    const reached = new Set<DepPath>()
    const seedDepPaths = nonListing.flatMap((index) => importerDirectDepPaths(importers[index]))
    walkAllEdges(lockfile, seedDepPaths, reached)
    applyNonListingGroupEdges(edges, groupEntries, reached)
  }
  return edges
}

function getRootDevDepPaths (lockfile: LockfileObject, resolvePeersFromWorkspaceRoot: boolean): Set<DepPath> | undefined {
  if (!resolvePeersFromWorkspaceRoot) return undefined
  const rootImporter = lockfile.importers['.' as ProjectId]
  return rootImporter == null ? undefined : new Set(resolvedDepsToDepPaths(rootImporter.devDependencies))
}

function groupEntriesByNonListing (
  entries: OptionalPeerEntry[],
  context: NonListingContext
): Array<{ nonListing: number[], entries: OptionalPeerEntry[] }> {
  const nonListingByTarget = new Map<DepPath, number[]>()
  const entriesByNonListing = new Map<string, { nonListing: number[], entries: OptionalPeerEntry[] }>()
  for (const entry of entries) {
    let nonListing = nonListingByTarget.get(entry.target)
    if (nonListing == null) {
      nonListing = findNonListingImporters(entry.target, context)
      nonListingByTarget.set(entry.target, nonListing)
    }
    const key = nonListing.join(',')
    let group = entriesByNonListing.get(key)
    if (group == null) {
      group = { nonListing, entries: [] }
      entriesByNonListing.set(key, group)
    }
    group.entries.push(entry)
  }
  return [...entriesByNonListing.values()]
}

function findNonListingImporters (target: DepPath, context: NonListingContext): number[] {
  if (context.rootDevDepPaths?.has(target)) return []
  return context.directDepPathsByImporter.flatMap((direct, index) => direct.has(target) ? [] : [index])
}

function applyNonListingGroupEdges (
  edges: Map<DepPath, Set<string>>,
  groupEntries: OptionalPeerEntry[],
  reached: Set<DepPath>
): void {
  for (const { parent, alias } of groupEntries) {
    if (reached.has(parent)) continue
    let aliases = edges.get(parent)
    if (aliases == null) {
      aliases = new Set()
      edges.set(parent, aliases)
    }
    aliases.add(alias)
  }
}

function collectOptionalPeerEntries (lockfile: LockfileObject): OptionalPeerEntry[] {
  const entries: OptionalPeerEntry[] = []
  for (const [parent, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    collectSnapshotOptionalPeerEntries(parent, snapshot, entries)
  }
  return entries
}

function collectSnapshotOptionalPeerEntries (
  parent: DepPath,
  snapshot: PackageSnapshot,
  entries: OptionalPeerEntry[]
): void {
  if (snapshot.peerDependencies == null || snapshot.peerDependenciesMeta == null) return
  collectDepsOptionalPeerEntries(parent, snapshot, snapshot.dependencies, entries)
  collectDepsOptionalPeerEntries(parent, snapshot, snapshot.optionalDependencies, entries)
}

function collectDepsOptionalPeerEntries (
  parent: DepPath,
  snapshot: PackageSnapshot,
  deps: ResolvedDependencies | undefined,
  entries: OptionalPeerEntry[]
): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    if (!isOptionalPeer(snapshot, alias)) continue
    const target = dp.refToRelative(ref, alias)
    if (target != null) entries.push({ parent, alias, target })
  }
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
    pushSnapshotDependencies(packages, depPath, stack)
  }
}

function pushSnapshotDependencies (
  packages: PackageSnapshots,
  depPath: DepPath,
  stack: DepPath[]
): void {
  const snapshot = Object.hasOwn(packages, depPath) ? packages[depPath] : undefined
  if (snapshot == null) return
  for (const child of resolvedDepsToDepPaths(snapshot.dependencies)) stack.push(child)
  for (const child of resolvedDepsToDepPaths(snapshot.optionalDependencies)) stack.push(child)
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

