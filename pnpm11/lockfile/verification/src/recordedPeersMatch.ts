import { parse, refToRelative } from '@pnpm/deps.path'
import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import { getPeerSet } from '@pnpm/installing.deps-resolver'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import type { DependencyManifest } from '@pnpm/types'
import semver from 'semver'

export interface RecordedPeersContext {
  /** Whether the package is declared by path, so its `catalog:` peers are checked against the locked provider. */
  declaredByPath: boolean
  /** The aliases in the package's scope. A peer in scope binds to that provider whatever the range. */
  parentPkgAliases: Record<string, unknown>
  resolveCatalogSpec: (peer: { alias: string, bareSpecifier: string }) => string | undefined
}

/**
 * The lockfile records the peers the package declares. A lockfile written
 * before it recorded every declared peer records only the peers taken without
 * any shadowing, the ones that are not also `dependencies`.
 *
 * The lockfile records a `catalog:` peer as written. A taken peer that no
 * dependency in scope provides is auto-installed from the catalog range, so
 * its locked provider must still satisfy that range.
 */
export function recordedPeersMatch (
  manifest: DependencyManifest,
  pkgSnapshot: PackageSnapshot,
  ctx: RecordedPeersContext
): boolean {
  const { declared, taken } = getPeerSet(manifest)
  const recorded = pkgSnapshot.peerDependencies ?? {}
  return Object.keys(taken).every((name) => Object.hasOwn(recorded, name)) &&
    Object.keys(recorded).every((name) => {
      const peer = declared[name]
      return peer != null &&
        peerRangeMatches(ctx, { alias: name, bareSpecifier: peer.version }, recorded[name]) &&
        (!Object.hasOwn(taken, name) || autoInstalledPeerSatisfiesCatalog(ctx, pkgSnapshot, { alias: name, bareSpecifier: peer.version })) &&
        Boolean(peer.optional) === Boolean(pkgSnapshot.peerDependenciesMeta?.[name]?.optional)
    })
}

function peerRangeMatches (
  ctx: RecordedPeersContext,
  peer: { alias: string, bareSpecifier: string },
  recordedRange: string | undefined
): boolean {
  if (recordedRange == null) return false
  return recordedRange === peer.bareSpecifier || recordedRange === ctx.resolveCatalogSpec(peer)
}

function autoInstalledPeerSatisfiesCatalog (
  ctx: RecordedPeersContext,
  pkgSnapshot: PackageSnapshot,
  peer: { alias: string, bareSpecifier: string }
): boolean {
  if (!ctx.declaredByPath || !peer.bareSpecifier.startsWith('catalog:') || Object.hasOwn(ctx.parentPkgAliases, peer.alias)) return true
  const catalogSpec = ctx.resolveCatalogSpec(peer)
  const range = catalogSpec == null ? null : semver.validRange(getPeerVersionRange(catalogSpec), { loose: true })
  const providerVersion = lockedPeerVersion(pkgSnapshot, peer.alias)
  if (range == null || providerVersion == null) return true
  return semver.satisfies(providerVersion, range, { includePrerelease: true, loose: true })
}

/** The version of the package the snapshot's peer resolves to, which the snapshot records among its dependencies. */
function lockedPeerVersion (pkgSnapshot: PackageSnapshot, alias: string): string | undefined {
  const ref = pkgSnapshot.dependencies?.[alias] ?? pkgSnapshot.optionalDependencies?.[alias]
  const depPath = ref == null ? null : refToRelative(ref, alias)
  return depPath == null ? undefined : parse(depPath).version
}
