import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import type { PatchInfo } from '@pnpm/patching.types'
import type { PackageResponse } from '@pnpm/store.controller-types'
import { DEPENDENCIES_OR_PEER_FIELDS, type PackageManifest, type PkgIdWithPatchHash } from '@pnpm/types'

import type { WantedDependency } from './getWantedDependencies.js'
import type { InfoFromLockfile, PeerDependencies, ResolvedPackage } from './resolutionTypes.js'

export function getManifestFromResponse (
  pkgResponse: PackageResponse,
  wantedDependency: WantedDependency,
  currentPkg?: Partial<InfoFromLockfile>
): PackageManifest {
  if (pkgResponse.body.manifest) return pkgResponse.body.manifest

  if (currentPkg?.name && currentPkg?.version && currentPkg.version !== '0.0.0') {
    return {
      name: currentPkg.name,
      version: currentPkg.version,
    }
  }
  return {
    name: wantedDependency.alias ? wantedDependency.alias : wantedDependency.bareSpecifier.split('/').pop()!,
  } as PackageManifest
}

/**
 * Returns a manifest that resolution may write to freely, leaving `manifest`
 * untouched down to each `peerDependenciesMeta` entry. Every other field is
 * shared with `manifest` and must stay read-only.
 *
 * The resolver returns the manifest object its metadata cache holds, so every
 * dependency that resolves to the same package version is handed the same
 * object. What resolution writes to it decides the isolation this owes:
 * dependency and peer records are rewritten by the read-package hook, a
 * `deprecated` notice is carried over from the lockfile, and an
 * `engines.runtime` entry becomes a dependency. `dependencies` is present on
 * the result whether or not the manifest declares it, since the peer handling
 * and `convertEnginesRuntimeToDependencies` both write into it.
 */
export function copyResolvedManifest (manifest: PackageManifest): PackageManifest {
  const copy: PackageManifest = { ...manifest, dependencies: { ...manifest.dependencies } }
  for (const depsField of DEPENDENCIES_OR_PEER_FIELDS) {
    if (manifest[depsField] != null) {
      copy[depsField] = { ...manifest[depsField] }
    }
  }
  if (manifest.peerDependenciesMeta != null) {
    copy.peerDependenciesMeta = {}
    for (const [peerName, peerMeta] of Object.entries(manifest.peerDependenciesMeta)) {
      copy.peerDependenciesMeta[peerName] = { ...peerMeta }
    }
  }
  return copy
}

export function pkgIsLeaf (pkg: PackageManifest): boolean {
  return Object.keys(pkg.dependencies ?? {}).length === 0 &&
    Object.keys(pkg.optionalDependencies ?? {}).length === 0 &&
    Object.keys(pkg.peerDependencies ?? {}).length === 0 &&
    // Package manifests can declare peerDependenciesMeta without declaring
    // peerDependencies. peerDependenciesMeta implies the later.
    Object.keys(pkg.peerDependenciesMeta ?? {}).length === 0
}

export function getResolvedPackage (
  options: {
    dependencyLockfile?: PackageSnapshot
    pkgIdWithPatchHash: PkgIdWithPatchHash
    force: boolean
    hasBin: boolean
    parentImporterId: string
    patch?: PatchInfo
    pkg: PackageManifest
    pkgResponse: PackageResponse
    prepare?: boolean
    optional: boolean
    wantedDependency: WantedDependency
  }
): ResolvedPackage {
  const peerDependencies = peerDependenciesWithoutOwn(options.pkg)

  return {
    additionalInfo: {
      bundledDependencies: options.pkg.bundledDependencies,
      bundleDependencies: options.pkg.bundleDependencies,
      cpu: options.pkg.cpu,
      deprecated: options.pkg.deprecated,
      engines: options.pkg.engines,
      os: options.pkg.os,
      libc: options.pkg.libc,
    },
    isLeaf: pkgIsLeaf(options.pkg),
    pkgIdWithPatchHash: options.pkgIdWithPatchHash,
    dev: options.wantedDependency.dev,
    fetching: options.pkgResponse.fetching!,
    resolutionNeedsFetch: options.pkgResponse.resolutionNeedsFetch,
    filesIndexFile: options.pkgResponse.filesIndexFile!,
    hasBin: options.hasBin,
    hasBundledDependencies: !((options.pkg.bundledDependencies ?? options.pkg.bundleDependencies) == null),
    id: options.pkgResponse.body.id,
    name: options.pkg.name,
    optional: options.optional,
    optionalDependencies: new Set(Object.keys(options.pkg.optionalDependencies ?? {})),
    patch: options.patch,
    peerDependencies,
    prepare: options.prepare,
    prod: !options.wantedDependency.dev && !options.wantedDependency.optional,
    resolution: options.pkgResponse.body.resolution,
    resolvedVia: options.pkgResponse.body.resolvedVia,
    version: options.pkg.version,
  }
}

/**
 * Throw when two different artifacts have collapsed onto one resolution id
 * because a named registry served a `name@version` another registry already
 * provided.
 *
 * Only reachable while the lockfile 12.0 format is off: with it on, a
 * named-registry package is keyed `<name>@<registryName>:<version>` and
 * cannot collide. Without the qualifier the second resolution silently
 * reuses the first one's tarball, so the dependency that asked for the
 * named registry gets the other registry's bytes.
 * The check is limited to named-registry involvement so nothing else can trip
 * it, and within that it is fail-closed: the two are allowed to share an id
 * only when something positively proves they are the same artifact — equal
 * integrity, or failing that an equal tarball URL. Being unable to tell is
 * treated as a collision, because the alternative is handing one dependency
 * the other registry's bytes.
 */
export function detectNamedRegistryCollision (
  resolved: ResolvedPackage,
  pkgResponse: PackageResponse
): void {
  if (resolved.resolvedVia !== 'named-registry' && pkgResponse.body.resolvedVia !== 'named-registry') return
  const existing = resolved.resolution as TarballResolution | undefined
  const incoming = pkgResponse.body.resolution as TarballResolution | undefined
  if (typeof existing?.integrity === 'string' && typeof incoming?.integrity === 'string') {
    if (existing.integrity === incoming.integrity) return
  } else if (
    typeof existing?.tarball === 'string' &&
    typeof incoming?.tarball === 'string' &&
    existing.tarball === incoming.tarball
  ) {
    return
  }
  throw new PnpmError(
    'NAMED_REGISTRY_PACKAGE_COLLISION',
    `"${resolved.name}@${resolved.version}" resolved to two different artifacts under one identity.`,
    {
      hint: 'A registry served different content for the same package name and version. Continuing would hand one dependency the other artifact\'s bytes, so the install stops here.',
    }
  )
}

export function detectRegistryRevisionConflict (
  resolved: ResolvedPackage,
  pkgResponse: PackageResponse
): void {
  const existing = resolved.resolution as TarballResolution | undefined
  const incoming = pkgResponse.body.resolution as TarballResolution | undefined
  if (existing?.revision == null && incoming?.revision == null) return
  if (existing?.revision === incoming?.revision && existing?.integrity === incoming?.integrity) return
  throw new PnpmError(
    'REVISION_CONFLICT',
    `Conflicting registry revisions were requested for "${resolved.name}@${resolved.version}".`,
    {
      hint: 'A single package name and version can resolve to only one registry artifact in an install.',
    }
  )
}

function peerDependenciesWithoutOwn (pkg: PackageManifest): PeerDependencies {
  if ((pkg.peerDependencies == null) && (pkg.peerDependenciesMeta == null)) return {}
  const ownDeps = new Set([
    pkg.name,
    ...Object.keys(pkg.dependencies ?? {}),
    ...Object.keys(pkg.optionalDependencies ?? {}),
  ])
  const result: PeerDependencies = {}
  for (const [peerName, peerRange] of Object.entries(pkg.peerDependencies ?? {})) {
    if (ownDeps.has(peerName)) continue
    result[peerName] = {
      version: peerRange,
    }
  }
  if (pkg.peerDependenciesMeta != null) {
    markOptionalPeers(result, pkg.peerDependenciesMeta, ownDeps)
  }
  return result
}

function markOptionalPeers (
  peerDependencies: PeerDependencies,
  peerDependenciesMeta: NonNullable<PackageManifest['peerDependenciesMeta']>,
  ownDeps: Set<string>
): void {
  for (const [peerName, peerMeta] of Object.entries(peerDependenciesMeta)) {
    if (ownDeps.has(peerName) || peerMeta.optional !== true) continue
    if (!peerDependencies[peerName]) peerDependencies[peerName] = { version: '*' }
    peerDependencies[peerName].optional = true
  }
}
