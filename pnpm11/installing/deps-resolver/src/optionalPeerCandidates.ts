import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { PkgResolutionId, PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { PackageManifest } from '@pnpm/types'
import * as semverUtils from '@yarnpkg/core/semverUtils'
import semver from 'semver'

import type { MissingPeerInfo, PeerDependencies, PkgAddressOrLink, ResolutionContext, ResolvedPackage, ResolvedPkgsById } from './resolutionTypes.js'
import { getPeerSet } from './resolvedPackage.js'

/** The version each direct dependency alias resolved to. */
export function getDirectDepVersions (
  resolvedPkgsById: ResolvedPkgsById,
  pkgAddresses: PkgAddressOrLink[]
): Map<string, string> {
  const versions = new Map<string, string>()
  for (const pkgAddress of pkgAddresses) {
    const version = getResolvedVersion(resolvedPkgsById, pkgAddress)
    if (version != null && !versions.has(pkgAddress.alias)) {
      versions.set(pkgAddress.alias, version)
    }
  }
  for (const pkgAddress of pkgAddresses) {
    const realName = pkgAddress.isLinkedDependency
      ? pkgAddress.name
      : resolvedPkgsById[pkgAddress.pkgId]?.name
    const version = getResolvedVersion(resolvedPkgsById, pkgAddress)
    if (realName && realName !== pkgAddress.alias && !versions.has(realName) && version != null) {
      versions.set(realName, version)
    }
  }
  return versions
}

function getResolvedVersion (resolvedPkgsById: ResolvedPkgsById, pkgAddress: PkgAddressOrLink): string | undefined {
  return pkgAddress.isLinkedDependency
    ? pkgAddress.version
    : resolvedPkgsById[pkgAddress.pkgId]?.version
}

/** The peers an optional peer candidate declares. */
export type CandidatePeers = Record<string, MissingPeerInfo>

/**
 * Looks up the peers an optional peer candidate declares. A candidate is
 * found by name and version, since a package resolved from a named registry
 * has a different ID. One seeded only from the wanted lockfile has no resolved
 * package yet, so the lockfile describes its peers instead. The name and
 * version index is built on the first miss, once per hoisting round.
 */
export function createCandidatePeersLookup (
  ctx: Pick<ResolutionContext, 'resolvedPkgsById' | 'wantedLockfile'>
): (name: string, version: string) => CandidatePeers | undefined {
  let byNameVersion: Map<string, CandidatePeers> | undefined
  return (name, version) => {
    const pkgId = `${name}@${version}`
    const resolvedPackage = ctx.resolvedPkgsById[pkgId as PkgResolutionId]
    if (resolvedPackage != null) return getPeers(resolvedPackage)
    byNameVersion ??= indexPeersByNameVersion(ctx)
    return byNameVersion.get(pkgId)
  }
}

function indexPeersByNameVersion (
  ctx: Pick<ResolutionContext, 'resolvedPkgsById' | 'wantedLockfile'>
): Map<string, CandidatePeers> {
  const byNameVersion = new Map<string, CandidatePeers>()
  for (const [depPath, pkgSnapshot] of Object.entries(ctx.wantedLockfile.packages ?? {})) {
    const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    if (pkgName && pkgVersion) {
      byNameVersion.set(`${pkgName}@${pkgVersion}`, getLockfilePeers(pkgSnapshot))
    }
  }
  for (const pkg of Object.values(ctx.resolvedPkgsById)) {
    byNameVersion.set(`${pkg.name}@${pkg.version}`, getPeers(pkg))
  }
  return byNameVersion
}

function getLockfilePeers ({ peerDependencies, peerDependenciesMeta }: PackageSnapshot): CandidatePeers {
  const peers: CandidatePeers = {}
  for (const [peerName, range] of Object.entries(peerDependencies ?? {})) {
    peers[peerName] = { range, optional: peerDependenciesMeta?.[peerName]?.optional === true }
  }
  return peers
}

function getPeers (resolvedPackage: ResolvedPackage): CandidatePeers {
  return toCandidatePeers(resolvedPackage.peers.taken)
}

/** The peers a package resolved from `manifest` takes. */
export function getManifestPeers (manifest: PackageManifest): CandidatePeers {
  return toCandidatePeers(getPeerSet(manifest).taken)
}

function toCandidatePeers (peers: PeerDependencies): CandidatePeers {
  return Object.fromEntries(Object.entries(peers).map(([peerName, { version: range, optional }]) => [
    peerName,
    { range, optional: optional === true },
  ]))
}

/**
 * The required peers of optional peer candidates that are resolved only to
 * read their manifests, without adding them to the graph.
 */
export interface DetachedPeerResolutions {
  /** The specifiers to resolve, keyed by the `name@spec` they were hoisted at. */
  pending: Map<string, { name: string, spec: string }>
  /** The peers of each resolved package, keyed by `name@version`. */
  peers: Map<string, CandidatePeers>
  /** The version each `name@spec` resolved to, `null` when it did not resolve. */
  versions: Map<string, string | null>
}

export function createDetachedPeerResolutions (): DetachedPeerResolutions {
  return { pending: new Map(), peers: new Map(), versions: new Map() }
}

/**
 * The version a required peer hoisted at `spec` installs, or `null` when it
 * does not resolve. A spec whose version or peers are unknown is added to
 * `detached.pending`, and the pick is its exact version, if it has one.
 */
export function pickHoistedVersion (
  detached: DetachedPeerResolutions,
  { knowsPeers, name, spec }: {
    knowsPeers: (name: string, version: string) => boolean
    name: string
    spec: string
  }
): string | null | undefined {
  const exactVersion = semver.valid(spec.split(' || ')[0]) ?? undefined
  if (exactVersion != null && knowsPeers(name, exactVersion)) return exactVersion
  const key = `${name}@${spec}`
  if (detached.versions.has(key)) return detached.versions.get(key)
  detached.pending.set(key, { name, spec: exactVersion ?? spec })
  return exactVersion
}

/**
 * The versions `pickVersion` picks for the required peers in `merged`, the
 * merge of `missingPeersOfWave`. `undefined` when a required peer would stay
 * unmet, as the merge drops it or `pickVersion` finds it does not resolve.
 */
export function pickMergedPeerVersions ({ merged, missingPeersOfWave, pickVersion }: {
  merged: CandidatePeers
  missingPeersOfWave: CandidatePeers[]
  pickVersion: (name: string, range: string) => string | null | undefined
}): Map<string, string> | undefined {
  const dropsAnyPeer = missingPeersOfWave.some((missingPeers) =>
    Object.entries(missingPeers).some(([name, { optional }]) => !optional && merged[name] == null))
  if (dropsAnyPeer) return undefined
  const versions = new Map<string, string>()
  for (const [name, { range, optional }] of Object.entries(merged)) {
    if (optional) continue
    const version = pickVersion(name, range)
    if (version === null) return undefined
    if (version != null) versions.set(name, version)
  }
  return versions
}

/**
 * Whether hoisting an optional peer candidate to an importer leaves every
 * peer it brings in satisfied. The candidate resolves its peers from
 * `providedVersions`. The required peers missing there are hoisted together,
 * one wave at a time, at the versions `pickRequiredPeerVersions` returns,
 * and resolve their own peers the same way. A wave that rejects a version
 * already known stops the check before the next wave is picked.
 */
export function acceptsOptionalPeerCandidate (
  { getCandidatePeers, pickRequiredPeerVersions, providedVersions }: {
    getCandidatePeers: (name: string, version: string) => CandidatePeers | undefined
    /** Picks the versions to hoist for the missing peers that each package of a wave declares, or `undefined` when a required one conflicts. */
    pickRequiredPeerVersions: (missingPeersOfWave: CandidatePeers[]) => Map<string, string> | undefined
    providedVersions: Map<string, string>
  },
  candidate: { name: string, version: string }
): boolean {
  const hoisted = new Map([[candidate.name, candidate.version]])
  const versionOf = (name: string): string | undefined => hoisted.get(name) ?? providedVersions.get(name)
  const peersOfHoisted: CandidatePeers[] = []
  let wave = [candidate]
  while (wave.length) {
    const peersOfWave = wave.map(({ name, version }) => getCandidatePeers(name, version)).filter((peers) => peers != null)
    if (!peersOfWave.every((peers) => peersAcceptVersions(peers, versionOf))) return false
    peersOfHoisted.push(...peersOfWave)
    const picked = pickRequiredPeerVersions(peersOfWave.map((peers) => pickMissingPeers(peers, versionOf)))
    if (picked == null) return false
    wave = []
    for (const [name, version] of picked) {
      hoisted.set(name, version)
      wave.push({ name, version })
    }
  }
  return peersOfHoisted.every((peers) => peersAcceptVersions(peers, versionOf))
}

function pickMissingPeers (peers: CandidatePeers, versionOf: (name: string) => string | undefined): CandidatePeers {
  return Object.fromEntries(Object.entries(peers).filter(([name]) => versionOf(name) == null))
}

function peersAcceptVersions (
  peers: CandidatePeers,
  versionOf: (name: string) => string | undefined
): boolean {
  return Object.entries(peers).every(([peerName, { range }]) => {
    const version = versionOf(peerName)
    return version == null || semverUtils.satisfiesWithPrereleases(version, getPeerVersionRange(range), true)
  })
}

/**
 * The `preferredVersions` of `names` that the importer's own dependency
 * closure resolves, or that `extraVersions` lists. A version that only
 * another importer resolves is left out.
 */
export function pickVersionsInClosure (
  ctx: Pick<ResolutionContext, 'childrenByParentId' | 'resolvedPkgsById'>,
  { extraVersions, names, preferredVersions, roots }: {
    extraVersions: Iterable<[string, string]>
    names: Set<string>
    preferredVersions: PreferredVersions
    roots: Array<Pick<PkgAddressOrLink, 'pkgId'>>
  }
): PreferredVersions {
  const closureVersions = new Map<string, Set<string>>()
  for (const [name, version] of [...extraVersions, ...listClosureVersions(ctx, roots)]) {
    if (!names.has(name)) continue
    const versions = closureVersions.get(name) ?? new Set()
    versions.add(version)
    closureVersions.set(name, versions)
  }
  const picked = Object.create(null) as PreferredVersions
  for (const [name, versions] of closureVersions) {
    const kept = Object.fromEntries(Object.entries(preferredVersions[name] ?? {}).filter(([version]) => versions.has(version)))
    if (Object.keys(kept).length > 0) picked[name] = kept
  }
  return picked
}

function listClosureVersions (
  ctx: Pick<ResolutionContext, 'childrenByParentId' | 'resolvedPkgsById'>,
  roots: Array<Pick<PkgAddressOrLink, 'pkgId'>>
): Array<[string, string]> {
  const nameVersions: Array<[string, string]> = []
  const visited = new Set<PkgResolutionId>()
  const pending = roots.map(({ pkgId }) => pkgId)
  while (pending.length) {
    const pkgId = pending.pop()!
    const pkg = ctx.resolvedPkgsById[pkgId]
    if (visited.has(pkgId) || pkg == null) continue
    visited.add(pkgId)
    nameVersions.push([pkg.name, pkg.version])
    for (const child of ctx.childrenByParentId[pkgId] ?? []) {
      if (child.linked != null) {
        nameVersions.push([child.linked.name, child.linked.version])
      } else {
        pending.push(child.id)
      }
    }
  }
  return nameVersions
}

/** `accepts`, adding the name of every candidate it rejects to `rejected`. */
export function recordingRejections (
  accepts: (name: string, version: string) => boolean,
  rejected: Set<string>
): (name: string, version: string) => boolean {
  return (name, version) => {
    const accepted = accepts(name, version)
    if (!accepted) rejected.add(name)
    return accepted
  }
}
