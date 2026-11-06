import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { PkgResolutionId, PreferredVersions } from '@pnpm/resolving.resolver-base'
import * as semverUtils from '@yarnpkg/core/semverUtils'

import type { MissingPeerInfo, PkgAddressOrLink, ResolutionContext, ResolvedPackage, ResolvedPkgsById } from './resolutionTypes.js'

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
  return Object.fromEntries(Object.entries(resolvedPackage.peers.taken).map(([peerName, { version: range, optional }]) => [
    peerName,
    { range, optional: optional === true },
  ]))
}

/**
 * Whether hoisting an optional peer candidate to an importer leaves every
 * peer it brings in satisfied. The candidate resolves its peers from
 * `providedVersions`. Each required peer missing there is hoisted at the
 * version `pickRequiredPeerVersion` returns, and resolves its own peers the
 * same way.
 */
export function acceptsOptionalPeerCandidate (
  { getCandidatePeers, pickRequiredPeerVersion, providedVersions }: {
    getCandidatePeers: (name: string, version: string) => CandidatePeers | undefined
    pickRequiredPeerVersion: (name: string, range: string) => string | undefined
    providedVersions: Map<string, string>
  },
  candidate: { name: string, version: string }
): boolean {
  const provided = new Map(providedVersions).set(candidate.name, candidate.version)
  const peersOfHoisted: CandidatePeers[] = []
  const pending = [candidate]
  while (pending.length) {
    const { name, version } = pending.pop()!
    const peers = getCandidatePeers(name, version)
    if (peers == null) continue
    peersOfHoisted.push(peers)
    pending.push(...hoistMissingRequiredPeers(peers, { pickRequiredPeerVersion, provided }))
  }
  return peersOfHoisted.every((peers) => peersAcceptProvidedVersions(peers, provided))
}

/** Adds to `provided` each required peer of `peers` it misses and that is hoisted, and returns them. */
function hoistMissingRequiredPeers (
  peers: CandidatePeers,
  { pickRequiredPeerVersion, provided }: {
    pickRequiredPeerVersion: (name: string, range: string) => string | undefined
    provided: Map<string, string>
  }
): Array<{ name: string, version: string }> {
  const hoisted: Array<{ name: string, version: string }> = []
  for (const [name, { range, optional }] of Object.entries(peers)) {
    if (optional || provided.has(name)) continue
    const version = pickRequiredPeerVersion(name, range)
    if (version == null) continue
    provided.set(name, version)
    hoisted.push({ name, version })
  }
  return hoisted
}

/**
 * An optional peer provider taken from elsewhere in the graph resolves its own
 * peers from the importer it is hoisted to. When the importer already has one
 * of those peers at a version outside the provider's range, hoisting the
 * provider creates a peer conflict the importer never asked for.
 */
function peersAcceptProvidedVersions (
  peers: CandidatePeers,
  providedVersions: Map<string, string>
): boolean {
  for (const [peerName, { range }] of Object.entries(peers)) {
    const providedVersion = providedVersions.get(peerName)
    if (providedVersion != null && !semverUtils.satisfiesWithPrereleases(providedVersion, getPeerVersionRange(range), true)) {
      return false
    }
  }
  return true
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
