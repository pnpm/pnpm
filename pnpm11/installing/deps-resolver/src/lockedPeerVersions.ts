import { getPeerBindingProviders, parsePeerBindings, refToRelative, removePeersSuffix } from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

type LockedPeer = [peerName: string, provider: DepPath]

/**
 * The peer versions the wanted lockfile pinned, by peer name: those on the
 * importer's direct dependencies, or on every snapshot for an importer the
 * lockfile does not list. The optional-peer hoist prefers these versions.
 */
export function getImporterLockedPeerVersions (lockfile: LockfileObject, importerId: ProjectId): Map<string, Set<string>> {
  const versions = new Map<string, Set<string>>()
  const importer = lockfile.importers != null && Object.hasOwn(lockfile.importers, importerId)
    ? lockfile.importers[importerId]
    : undefined
  const depPaths: DepPath[] = importer == null
    ? Object.keys(lockfile.packages ?? {}) as DepPath[]
    : [importer.dependencies, importer.optionalDependencies, importer.devDependencies]
      .flatMap((deps) => Object.entries(deps ?? {}) as Array<[string, string]>)
      .map(([alias, ref]) => refToRelative(ref, alias))
      .filter((depPath): depPath is DepPath => depPath != null)
  for (const depPath of depPaths) {
    for (const [name, version] of getLockedPeerVersionsOfKey(lockfile, depPath)) {
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
function getLockedPeerVersionsOfKey (lockfile: LockfileObject, depPath: DepPath): Array<[string, string]> {
  const versions: Array<[string, string]> = []
  const visited = new Set<DepPath>()
  const pending = [depPath]
  while (pending.length > 0) {
    const key = pending.pop()!
    if (visited.has(key)) continue
    visited.add(key)
    for (const [name, provider] of getLockedPeerKeys(lockfile, key)) {
      const providerWithoutPeers = removePeersSuffix(provider)
      versions.push([name, providerWithoutPeers.substring(providerWithoutPeers.indexOf('@', 1) + 1)])
      if (providerWithoutPeers !== provider) pending.push(provider)
    }
  }
  return versions
}

function getLockedPeerKeys (lockfile: LockfileObject, depPath: DepPath): LockedPeer[] {
  const bindings = parsePeerBindings(depPath)
  if (bindings == null) return []
  const snapshot = lockfile.packages?.[depPath]
  if (bindings.kind === 'hashed') return getHashedPeerKeys(lockfile, snapshot)
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

/**
 * The peers a hashed suffix stands for: the package's declared peers, read
 * from its own edges, and its `transitivePeerDependencies`, read from the
 * descendants that declare them.
 */
function getHashedPeerKeys (lockfile: LockfileObject, snapshot: PackageSnapshot | undefined): LockedPeer[] {
  if (snapshot == null) return []
  const keys: LockedPeer[] = getPeerEdgeKeys(snapshot, (name) => declaresPeer(snapshot, name))
  for (const peerName of snapshot.transitivePeerDependencies ?? []) {
    for (const provider of getTransitivePeerProviders(lockfile, snapshot, peerName)) {
      keys.push([peerName, provider])
    }
  }
  return keys
}

function getTransitivePeerProviders (lockfile: LockfileObject, snapshot: PackageSnapshot, peerName: string): DepPath[] {
  const providers: DepPath[] = []
  const visited = new Set<DepPath>()
  const pending = [snapshot]
  while (pending.length > 0) {
    for (const child of getUnvisitedChildSnapshots(lockfile, pending.pop()!, visited)) {
      visitTransitivePeerChild(child, { peerName, pending, providers })
    }
  }
  return providers
}

/** A child that declares the peer holds the provider edge. One that lists it as a transitive peer is descended into. */
function visitTransitivePeerChild (
  child: PackageSnapshot,
  { peerName, pending, providers }: { peerName: string, pending: PackageSnapshot[], providers: DepPath[] }
): void {
  if (declaresPeer(child, peerName)) {
    providers.push(...getPeerEdgeKeys(child, (name) => name === peerName).map(([, provider]) => provider))
  } else if (child.transitivePeerDependencies?.includes(peerName)) {
    pending.push(child)
  }
}

function getUnvisitedChildSnapshots (lockfile: LockfileObject, snapshot: PackageSnapshot, visited: Set<DepPath>): PackageSnapshot[] {
  const children: PackageSnapshot[] = []
  for (const [edgeName, ref] of getDependencyEdges(snapshot)) {
    const childKey = refToRelative(ref, edgeName)
    const child = childKey == null ? undefined : lockfile.packages?.[childKey]
    if (childKey == null || child == null || visited.has(childKey)) continue
    visited.add(childKey)
    children.push(child)
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
