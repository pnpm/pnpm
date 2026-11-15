import { getPeerBindingProviders, parse, parsePeerBindings, refToRelative } from '@pnpm/deps.path'
import { getPeerSet } from '@pnpm/installing.deps-resolver'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.types'
import type { DependencyManifest, DepPath } from '@pnpm/types'
import semver from 'semver'

import { versionSatisfies } from './dependencySpec.js'

export function recordedPeersMatch (
  {
    manifest,
    pkgSnapshot,
    declaredByPath,
    depPathsBySnapshot,
    lockfilePackages,
    resolveCatalogSpec,
  }: {
    manifest: DependencyManifest
    pkgSnapshot: PackageSnapshot
    declaredByPath: boolean
    depPathsBySnapshot: Map<PackageSnapshot, DepPath>
    lockfilePackages: PackageSnapshots | undefined
    resolveCatalogSpec: (peer: { alias: string, bareSpecifier: string }) => string | undefined
  }
): boolean {
  const { declared, taken } = getPeerSet(manifest)
  const recorded = pkgSnapshot.peerDependencies ?? {}
  return Object.keys(taken).every((name) => Object.hasOwn(recorded, name)) &&
    Object.keys(recorded).every((name) => {
      const peer = declared[name]
      return peer != null &&
        peerRangeMatches({
          peer: { alias: name, bareSpecifier: peer.version },
          recordedRange: recorded[name],
          declaredByPath,
          pkgSnapshot,
          depPathsBySnapshot,
          lockfilePackages,
          resolveCatalogSpec,
        }) &&
        Boolean(peer.optional) === Boolean(pkgSnapshot.peerDependenciesMeta?.[name]?.optional)
    })
}

function peerRangeMatches (
  {
    peer,
    recordedRange,
    declaredByPath,
    pkgSnapshot,
    depPathsBySnapshot,
    lockfilePackages,
    resolveCatalogSpec,
  }: {
    peer: { alias: string, bareSpecifier: string }
    recordedRange: string | undefined
    declaredByPath: boolean
    pkgSnapshot: PackageSnapshot
    depPathsBySnapshot: Map<PackageSnapshot, DepPath>
    lockfilePackages: PackageSnapshots | undefined
    resolveCatalogSpec: (peer: { alias: string, bareSpecifier: string }) => string | undefined
  }
): boolean {
  if (recordedRange == null) return false
  if (!declaredByPath) return recordedRange === peer.bareSpecifier
  if (!peer.bareSpecifier.startsWith('catalog:')) return recordedRange === resolveCatalogSpec(peer)
  const catalogRange = resolveCatalogSpec(peer)
  if (catalogRange == null || catalogRange === peer.bareSpecifier) return recordedRange === peer.bareSpecifier
  return catalogPeerVersionSatisfies({
    alias: peer.alias,
    range: catalogRange,
    pkgSnapshot,
    depPathsBySnapshot,
    lockfilePackages,
  }) ?? (recordedRange === peer.bareSpecifier)
}

function catalogPeerVersionSatisfies (
  {
    alias,
    range,
    pkgSnapshot,
    depPathsBySnapshot,
    lockfilePackages,
  }: {
    alias: string
    range: string
    pkgSnapshot: PackageSnapshot
    depPathsBySnapshot: Map<PackageSnapshot, DepPath>
    lockfilePackages: PackageSnapshots | undefined
  }
): boolean | undefined {
  const depPath = depPathsBySnapshot.get(pkgSnapshot)
  if (depPath == null) return undefined
  const bindings = parsePeerBindings(depPath)
  const provider = bindings?.kind === 'explicit'
    ? getPeerBindingProviders(bindings).find((providerPath) => parse(providerPath).name === alias)
    : dependencyProviderPath(pkgSnapshot, alias, lockfilePackages)
  if (provider == null) return undefined
  const providerVersion = parse(provider).version
  if (providerVersion === range) return true
  if (providerVersion == null || semver.validRange(range) == null) return undefined
  return versionSatisfies(providerVersion, range)
}

function dependencyProviderPath (
  pkgSnapshot: PackageSnapshot,
  alias: string,
  lockfilePackages: PackageSnapshots | undefined
): string | undefined {
  const ref = pkgSnapshot.dependencies?.[alias] ?? pkgSnapshot.optionalDependencies?.[alias]
  const depPath = ref == null ? null : refToRelative(ref, alias)
  return depPath == null || lockfilePackages?.[depPath] == null ? undefined : depPath
}
