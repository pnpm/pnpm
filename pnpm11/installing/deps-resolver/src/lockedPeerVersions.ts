import { getBoundPeerProviders, getPeerBindingProviders, parsePeerBindings, refToRelative, removePeersSuffix } from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

import type { ResolutionContext } from './resolutionTypes.js'

/** Captures the manifest peer split after every importer's initial dependency resolution. */
export function createImporterLockedPeerVersions (
  { wantedLockfile, originalImporterIds, resolvedPkgsById }: Pick<ResolutionContext, 'wantedLockfile' | 'originalImporterIds' | 'resolvedPkgsById'>
): (importerId: ProjectId) => Map<string, Set<string>> {
  const takenPeers = new Map(Object.values(resolvedPkgsById).map((pkg) => [pkg.pkgIdWithPatchHash, new Set(Object.keys(pkg.peers.taken))]))
  const byImporter = new Map<ProjectId | undefined, Map<string, Set<string>>>()
  return (importerId) => {
    const listed = originalImporterIds?.has(importerId) ?? (wantedLockfile.importers != null && Object.hasOwn(wantedLockfile.importers, importerId))
    const key = listed ? importerId : undefined
    let versions = byImporter.get(key)
    if (versions == null) {
      versions = getImporterLockedPeerVersions(wantedLockfile, key, takenPeers)
      byImporter.set(key, versions)
    }
    return versions
  }
}

type LockedPeer = [peerName: string, provider: DepPath]

/**
 * The peer versions the wanted lockfile pinned, by peer name: those on the
 * importer's direct dependencies, or on every snapshot for an importer the
 * lockfile does not list.
 */
export function getImporterLockedPeerVersions (
  lockfile: LockfileObject,
  importerId: ProjectId | undefined,
  takenPeers?: ReadonlyMap<string, ReadonlySet<string>>
): Map<string, Set<string>> {
  const lookup: SnapshotLookup = { lockfile, children: new WeakMap(), takenPeers }
  const versions = new Map<string, Set<string>>()
  const importer = importerId != null && lockfile.importers != null && Object.hasOwn(lockfile.importers, importerId)
    ? lockfile.importers[importerId]
    : undefined
  const depPaths: DepPath[] = importer == null
    ? Object.keys(lockfile.packages ?? {}) as DepPath[]
    : [importer.dependencies, importer.optionalDependencies, importer.devDependencies]
      .flatMap((deps) => Object.entries(deps ?? {}) as Array<[string, string]>)
      .map(([alias, ref]) => refToRelative(ref, alias))
      .filter((depPath): depPath is DepPath => depPath != null)
  for (const depPath of depPaths) {
    for (const [name, version] of getLockedPeerVersionsOfKey(lookup, depPath)) {
      let bucket = versions.get(name)
      if (bucket == null) {
        bucket = new Set()
        versions.set(name, bucket)
      }
      bucket.add(version)
    }
  }
  return versions
}

/**
 * The peer name/version pairs the lockfile pinned for `depPath`, including
 * the peers of every peer provider the suffix names. An explicit suffix
 * spells them out, save for the names an npm alias renamed. A hashed suffix
 * spells out nothing, so the pairs are recovered from the snapshots.
 */
function getLockedPeerVersionsOfKey (lookup: SnapshotLookup, depPath: DepPath): Array<[string, string]> {
  const traversal: LockedPeerTraversal = { ...lookup, versions: [], visited: new Set(), recovered: [] }
  collectLockedPeerPins(traversal, [depPath], new Map())
  const encodedVersions = new Map<string, Set<string>>()
  for (const [name, version] of traversal.versions) {
    const versions = encodedVersions.get(name) ?? new Set<string>()
    versions.add(version)
    encodedVersions.set(name, versions)
  }
  while (traversal.recovered.length > 0) {
    const pending = traversal.recovered
    traversal.recovered = []
    collectLockedPeerPins(traversal, pending, encodedVersions)
  }
  return traversal.versions
}

interface SnapshotLookup {
  lockfile: LockfileObject
  children: WeakMap<PackageSnapshot, Array<[DepPath, PackageSnapshot]>>
  takenPeers?: ReadonlyMap<string, ReadonlySet<string>>
}

interface LockedPeerTraversal extends SnapshotLookup {
  versions: Array<[string, string]>
  visited: Set<DepPath>
  recovered: DepPath[]
}

function collectLockedPeerPins (traversal: LockedPeerTraversal, pending: DepPath[], encodedVersions: Map<string, Set<string>>): void {
  while (pending.length > 0) {
    const key = pending.pop()!
    if (traversal.visited.has(key)) continue
    traversal.visited.add(key)
    collectLockedPeerKey(traversal, { key, pending }, encodedVersions)
  }
}

function collectLockedPeerKey (
  traversal: LockedPeerTraversal,
  { key, pending }: { key: DepPath, pending: DepPath[] },
  encodedVersions: Map<string, Set<string>>
): void {
  const recoverContext = parsePeerBindings(key)?.kind !== 'hashed'
  for (const [name, provider] of getLockedPeerKeys(traversal, key)) {
    const providerWithoutPeers = removePeersSuffix(provider)
    if (providerWithoutPeers !== provider) pending.push(provider)
    const version = providerWithoutPeers.substring(providerWithoutPeers.indexOf('@', 1) + 1)
    const encoded = encodedVersions.get(name)
    if (encoded == null) traversal.versions.push([name, version])
    if (recoverContext && (encoded == null || encoded.has(version))) {
      traversal.recovered.push(...getSnapshotPeerProviders(traversal, key, { name, provider }))
    }
  }
}

function getSnapshotPeerProviders (
  lookup: SnapshotLookup,
  key: DepPath,
  { name, provider }: { name: string, provider: DepPath }
): DepPath[] {
  if (removePeersSuffix(provider) !== provider) return []
  const snapshot = lookup.lockfile.packages?.[key]
  if (snapshot == null) return []
  const reference = [snapshot.dependencies, snapshot.optionalDependencies]
    .find((dependencies) => dependencies != null && Object.hasOwn(dependencies, name))?.[name]
  const direct = reference == null ? null : refToRelative(reference, name)
  const providers = direct != null
    ? [direct]
    : snapshot.transitivePeerDependencies?.includes(name)
      ? getTransitivePeerProviders(lookup, snapshot, name)
      : []
  const base = removePeersSuffix(provider)
  return providers.filter((candidate) => candidate !== base && removePeersSuffix(candidate) === base)
}

function getLockedPeerKeys (lookup: SnapshotLookup, depPath: DepPath): LockedPeer[] {
  const bindings = parsePeerBindings(depPath)
  if (bindings == null) return []
  const snapshot = lookup.lockfile.packages?.[depPath]
  if (bindings.kind === 'hashed') return getHashedPeerKeys(lookup, depPath, snapshot)
  const providers = getPeerBindingProviders(bindings)
  const names = providers.map((provider) => provider.substring(0, provider.indexOf('@', 1)))
  if (snapshot != null) restoreAliasedPeerNames(snapshot, providers, names)
  return providers.map((provider, index) => [names[index], provider])
}

/**
 * Renames the providers an npm alias installs back to the name the dependent
 * installs them under. A peer suffix names a provider by the package it
 * resolved to, while peer resolution keys it by its name in the dependent's
 * `node_modules`.
 */
function restoreAliasedPeerNames (snapshot: PackageSnapshot, providers: DepPath[], names: string[]): void {
  const edges = getDependencyEdges(snapshot)
  if (!edges.some(([edgeName, ref]) => refToRelative(ref, edgeName) === ref)) return
  const aliasesByProvider = new Map<string, ProviderAliases>()
  for (const [edgeName, ref] of edges) {
    const target = refToRelative(ref, edgeName)
    if (target == null) continue
    const providerWithoutPeers = removePeersSuffix(target)
    let aliases = aliasesByProvider.get(providerWithoutPeers)
    if (aliases == null) {
      aliases = { declaredPeers: [], claimed: 0, ordinaries: 0 }
      aliasesByProvider.set(providerWithoutPeers, aliases)
    }
    recordProviderAlias(aliases, edgeName, declaresPeer(snapshot, edgeName))
  }
  providers.forEach((provider, index) => {
    const aliases = aliasesByProvider.get(removePeersSuffix(provider))
    const alias = aliases && claimProviderAlias(aliases)
    if (alias != null) names[index] = alias
  })
}

/** The names one provider is installed under, split by whether the dependent declares that name as a peer. */
interface ProviderAliases {
  declaredPeers: string[]
  claimed: number
  ordinary?: string
  ordinaries: number
}

function recordProviderAlias (aliases: ProviderAliases, alias: string, declaresPeer: boolean): void {
  if (declaresPeer) {
    aliases.declaredPeers.push(alias)
    return
  }
  aliases.ordinaries++
  if (aliases.ordinaries === 1) aliases.ordinary = alias
}

/**
 * Declared peer edges claim segments first. An ordinary edge takes the one
 * left over, unless several compete, which nothing in the lockfile tells
 * apart.
 */
function claimProviderAlias (aliases: ProviderAliases): string | undefined {
  if (aliases.claimed < aliases.declaredPeers.length) {
    return aliases.declaredPeers[aliases.claimed++]
  }
  if (aliases.ordinaries === 1) {
    const ordinary = aliases.ordinary
    aliases.ordinary = undefined
    return ordinary
  }
  return undefined
}

/** Candidate providers for the manifest's taken peers and the snapshot's transitive peers. */
function getHashedPeerKeys (lookup: SnapshotLookup, depPath: DepPath, snapshot: PackageSnapshot | undefined): LockedPeer[] {
  if (snapshot == null) return []
  const keys: LockedPeer[] = getPeerEdgeKeys(snapshot, (name) => takesPeer(lookup, depPath, name))
  for (const peerName of snapshot.transitivePeerDependencies ?? []) {
    for (const provider of getTransitivePeerProviders(lookup, snapshot, peerName)) {
      keys.push([peerName, provider])
    }
  }
  return keys
}

function getTransitivePeerProviders (lookup: SnapshotLookup, snapshot: PackageSnapshot, peerName: string): DepPath[] {
  const providers: DepPath[] = []
  const visited = new Set<DepPath>()
  const pending = [snapshot]
  while (pending.length > 0) {
    for (const child of getUnvisitedChildSnapshots(lookup, pending.pop()!, visited)) {
      visitTransitivePeerChild(lookup, child, { peerName, pending, providers })
    }
  }
  return providers
}

/** A child that binds the peer holds the provider edge. One that lists it as a transitive peer is descended into. */
function visitTransitivePeerChild (
  lookup: SnapshotLookup,
  [childKey, child]: [DepPath, PackageSnapshot],
  { peerName, pending, providers }: { peerName: string, pending: PackageSnapshot[], providers: DepPath[] }
): void {
  const childProviders = getBoundPeerEdgeProviders(lookup, [childKey, child], peerName)
  if (childProviders.length > 0) {
    providers.push(...childProviders)
  } else if (child.transitivePeerDependencies?.includes(peerName)) {
    pending.push(child)
  }
}

function getBoundPeerEdgeProviders (lookup: SnapshotLookup, [childKey, child]: [DepPath, PackageSnapshot], peerName: string): DepPath[] {
  if (!takesPeer(lookup, childKey, peerName)) return []
  const bindings = parsePeerBindings(childKey)
  if (bindings == null) return []
  const bound = getBoundPeerProviders(bindings)
  return getPeerEdgeKeys(child, (name) => name === peerName)
    .map(([, provider]) => provider)
    .filter((provider) => bound == null || bound.has(removePeersSuffix(provider)))
}

function getUnvisitedChildSnapshots (lookup: SnapshotLookup, snapshot: PackageSnapshot, visited: Set<DepPath>): Array<[DepPath, PackageSnapshot]> {
  const children = getChildSnapshots(lookup, snapshot)
  return children.filter(([childKey]) => {
    if (visited.has(childKey)) return false
    visited.add(childKey)
    return true
  })
}

function getChildSnapshots (lookup: SnapshotLookup, snapshot: PackageSnapshot): Array<[DepPath, PackageSnapshot]> {
  let children = lookup.children.get(snapshot)
  if (children == null) {
    children = []
    for (const [edgeName, ref] of getDependencyEdges(snapshot)) {
      const childKey = refToRelative(ref, edgeName)
      const child = childKey == null ? undefined : lookup.lockfile.packages?.[childKey]
      if (childKey != null && child != null) children.push([childKey, child])
    }
    lookup.children.set(snapshot, children)
  }
  return children
}

function getPeerEdgeKeys (snapshot: PackageSnapshot, isPeer: (name: string) => boolean): LockedPeer[] {
  const keys: LockedPeer[] = []
  for (const [name, ref] of getDependencyEdges(snapshot)) {
    if (!isPeer(name)) continue
    const provider = refToRelative(ref, name)
    if (provider != null) keys.push([name, provider])
  }
  return keys
}

function getDependencyEdges (snapshot: PackageSnapshot): Array<[string, string]> {
  return [
    ...Object.entries(snapshot.dependencies ?? {}),
    ...Object.entries(snapshot.optionalDependencies ?? {}),
  ]
}

// Own-property check: a lockfile is untrusted input, and a plain lookup also
// matches inherited Object.prototype names (`constructor`, `toString`, ...),
// some of which are valid package names.
function declaresPeer (snapshot: PackageSnapshot, name: string): boolean {
  return snapshot.peerDependencies != null && Object.hasOwn(snapshot.peerDependencies, name)
}

function takesPeer (lookup: SnapshotLookup, depPath: DepPath, name: string): boolean {
  const taken = lookup.takenPeers?.get(removePeersSuffix(depPath))
  if (taken != null) return taken.has(name)
  const snapshot = lookup.lockfile.packages?.[depPath]
  return snapshot != null && declaresPeer(snapshot, name)
}
