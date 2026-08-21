
import { isError, PnpmError } from '@pnpm/error'
import { filterPkgMetadataByPublishDate } from '@pnpm/resolving.registry.pkg-metadata-filter'
import type { PackageInRegistry, PackageMeta, PackageMetaWithTime } from '@pnpm/resolving.registry.types'
import type { VersionSelectors, VersionSelectorType } from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'

import { getDominantLockfileVersion, preferredSelectorInfo } from './dominantLockfileVersion.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import { parseModifiedDate } from './publishPolicy.js'
import {
  maxSatisfyingLoose,
  maxVersionLoose,
  minSatisfyingLoose,
  parseSemverLoose,
  semverSatisfiesLoose,
} from './semverLoose.js'

export {
  cachedMetaMissesPreferredVersion,
  getDominantLockfileVersion,
} from './dominantLockfileVersion.js'
export {
  findNonDeprecatedAlternative,
  installableUnderPolicy,
  knownImmature,
  type PublishPolicyOptions,
} from './publishPolicy.js'

export interface PickVersionByVersionRangeOptions {
  meta: PackageMeta
  versionRange: string
  preferredVersionSelectors?: VersionSelectors
  publishedBy?: Date
}

export type PickVersionByVersionRange = (options: PickVersionByVersionRangeOptions) => string | null

export interface PickPackageFromMetaOptions {
  preferredVersionSelectors: VersionSelectors | undefined
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
}

export function pickPackageFromMeta (
  pickVersionByVersionRangeFn: PickVersionByVersionRange,
  {
    preferredVersionSelectors,
    publishedBy,
    publishedByExclude,
  }: PickPackageFromMetaOptions,
  meta: PackageMeta,
  spec: RegistryPackageSpec
): PackageInRegistry | null {
  if (publishedBy) {
    meta = narrowMetaToPublishedBy(meta, publishedBy, publishedByExclude)
  }
  if ((!meta.versions || Object.keys(meta.versions).length === 0) && !publishedBy) {
    // Unfortunately, the npm registry doesn't return the time field in the abbreviated metadata.
    // So we won't always know if the package was unpublished.
    if (meta.time?.unpublished?.versions?.length) {
      throw new PnpmError('UNPUBLISHED_PKG', `No versions available for ${spec.name} because it was unpublished`)
    }
    throw new PnpmError('NO_VERSIONS', `No versions available for ${spec.name}. The package may be unpublished.`)
  }
  try {
    const version = pickVersionForSpec(pickVersionByVersionRangeFn, {
      meta,
      spec,
      preferredVersionSelectors,
      publishedBy,
    })
    if (!version) return null
    return readManifestUnderRegistryName(meta, version)
  } catch (err: unknown) {
    throw toMalformedMetadataError(err, spec)
  }
}

function narrowMetaToPublishedBy (
  meta: PackageMeta,
  publishedBy: Date,
  publishedByExclude: PackageVersionPolicy | undefined
): PackageMeta {
  const view = applyPublishedByPolicy(meta, publishedBy, publishedByExclude)
  if (view.needsFullMetadata) {
    const modifiedDate = parseModifiedDate(view.meta.modified)
    if (modifiedDate == null || modifiedDate > publishedBy) {
      // The package was modified after the cutoff (or carries no usable
      // `modified`), so which of its versions are mature is unknowable
      // from abbreviated metadata. The error tells the caller to refetch.
      assertMetaHasTime(view.meta)
    }
    // else: `modified` is an upper bound on every per-version timestamp, so
    // `modified <= publishedBy` means they all pass the maturity filter and
    // nothing would be dropped. Inclusive at the boundary on purpose, to
    // match the per-version `<=` in `filterPkgMetadataByPublishDate`.
  }
  return view.meta
}

function pickVersionForSpec (
  pickVersionByVersionRangeFn: PickVersionByVersionRange,
  { meta, spec, preferredVersionSelectors, publishedBy }: Omit<PickVersionByVersionRangeOptions, 'versionRange'> & { spec: RegistryPackageSpec }
): string | null | undefined {
  switch (spec.type) {
    case 'version':
      return spec.fetchSpec
    case 'tag':
      return meta['dist-tags'][spec.fetchSpec]
    case 'range':
      return pickVersionByVersionRangeFn({
        meta,
        versionRange: spec.fetchSpec,
        preferredVersionSelectors,
        publishedBy,
      })
  }
  return undefined
}

function readManifestUnderRegistryName (meta: PackageMeta, version: string): PackageInRegistry {
  const manifest = meta.versions[version]
  if (manifest && meta['name']) {
    // Packages that are published to the GitHub registry are always published with a scope.
    // However, the name in the package.json for some reason may omit the scope.
    // So the package published to the GitHub registry will be published under @foo/bar
    // but the name in package.json will be just bar.
    // In order to avoid issues, we consider that the real name of the package is the one with the scope.
    manifest.name = meta['name']
  }
  return manifest
}

function toMalformedMetadataError (err: unknown, spec: RegistryPackageSpec): Error {
  if (
    isError(err) &&
    'code' in err &&
    typeof err.code === 'string' &&
    err.code.startsWith('ERR_PNPM_')
  ) {
    return err
  }
  return new PnpmError('MALFORMED_METADATA',
    `Received malformed metadata for "${spec.name}"`,
    { hint: 'This might mean that the package was unpublished from the registry', cause: err }
  )
}

export interface PublishedByView {
  /** The metadata the cutoff leaves visible. `meta` itself when nothing is filtered out. */
  meta: PackageMeta
  /**
   * The cutoff could not be applied: the metadata is abbreviated, so there
   * are no per-version timestamps to filter on. Whether that is fatal is the
   * caller's call — the pick needs full metadata to honor the cutoff, while a
   * caller reasoning about a pick that already succeeded knows the versions
   * cleared the cutoff some other way.
   */
  needsFullMetadata: boolean
}

/**
 * Narrows `meta` to the versions the `publishedBy` cutoff admits, honoring
 * `publishedByExclude`: a package the policy excludes wholesale keeps its
 * unfiltered metadata, and versions the policy names explicitly stay in
 * regardless of their age.
 *
 * Every consumer of the cutoff goes through here so they agree on what the
 * policy admits — a baseline that filters differently from the pick would
 * misreport why a version was chosen.
 */
export function applyPublishedByPolicy (
  meta: PackageMeta,
  publishedBy: Date,
  publishedByExclude?: PackageVersionPolicy
): PublishedByView {
  const excludeResult = publishedByExclude?.(meta.name) ?? false
  if (excludeResult === true) return { meta, needsFullMetadata: false }
  if (meta.time == null) return { meta, needsFullMetadata: true }
  assertMetaHasTime(meta)
  const trustedVersions = Array.isArray(excludeResult) ? excludeResult : undefined
  return {
    meta: filterPkgMetadataByPublishDate(meta, publishedBy, trustedVersions),
    needsFullMetadata: false,
  }
}

export function assertMetaHasTime (meta: PackageMeta): asserts meta is PackageMetaWithTime {
  if (meta.time == null) {
    throw new PnpmError('MISSING_TIME', `The metadata of ${meta.name} is missing the "time" field`)
  }
}

export function pickLowestVersionByVersionRange (
  { meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions
): string | null {
  if (preferredVersionSelectors != null && Object.keys(preferredVersionSelectors).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
    for (const preferredVersions of prioritizedPreferredVersions) {
      const preferredVersion = minSatisfyingLoose(preferredVersions, versionRange)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }
  if (versionRange === '*') {
    return Object.keys(meta.versions).sort(semver.compare)[0]
  }
  return minSatisfyingLoose(Object.keys(meta.versions), versionRange)
}

export function pickVersionByVersionRange (options: PickVersionByVersionRangeOptions): string | null {
  const { meta, versionRange } = options
  const latest: string | undefined = meta['dist-tags'].latest

  const preferredVersion = pickPreferredVersionInRange(options, latest)
  if (preferredVersion != null) return preferredVersion

  if (latest && (versionRange === '*' || semverSatisfiesLoose(latest, versionRange))) {
    // Not using semver.satisfies in case of * because it does not select beta versions.
    // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
    if (!meta.versions[latest]?.deprecated) {
      return latest
    }
    const versions = Object.keys(meta.versions)
    return nonDeprecatedPick(meta, versions, latest, versionRange) ?? latest
  }

  const versions = Object.keys(meta.versions)
  const maxVersion = maxSatisfyingLoose(versions, versionRange)
  if (maxVersion) {
    return nonDeprecatedPick(meta, versions, maxVersion, versionRange) ?? maxVersion
  }
  return null
}

function pickPreferredVersionInRange (
  { meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions,
  latest: string | undefined
): string | null {
  if (preferredVersionSelectors == null || Object.keys(preferredVersionSelectors).length === 0) return null
  const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
  for (const preferredVersions of prioritizedPreferredVersions) {
    const preferredVersion = latest != null && preferredVersions.includes(latest) && semverSatisfiesLoose(latest, versionRange)
      ? latest
      : maxSatisfyingLoose(preferredVersions, versionRange)
    if (preferredVersion) {
      return nonDeprecatedPick(meta, preferredVersions, preferredVersion, versionRange) ?? preferredVersion
    }
  }
  return null
}

/**
 * Returns the cached version only when lockfile preferences prove that no
 * version missing from the cached packument could tie or outrank it.
 */
export function pickStableCachedRangeVersion ({
  meta,
  preferredVersionSelectors,
  versionRange,
}: PickVersionByVersionRangeOptions): string | null {
  const dominantLockfileVersion = getDominantLockfileVersion(versionRange, preferredVersionSelectors)
  if (dominantLockfileVersion == null || meta.versions[dominantLockfileVersion] == null) return null
  try {
    const pickedVersion = pickVersionByVersionRange({ meta, preferredVersionSelectors, versionRange })
    return pickedVersion === dominantLockfileVersion ? dominantLockfileVersion : null
  } catch {
    return null
  }
}

function prioritizePreferredVersions (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSelectors?: VersionSelectors
): string[][] {
  const preferredVerSelectorsArr = Object.entries(preferredVerSelectors ?? {})
  const versionsPrioritizer = new PreferredVersionsPrioritizer()

  // First, add all versions that satisfy versionRange with default weight 0
  for (const version of Object.keys(meta.versions)) {
    if (semverSatisfiesLoose(version, versionRange)) {
      versionsPrioritizer.add(version, 0)
    }
  }

  // Then apply weights from preferred selectors
  for (const [preferredSelector, preferredSelectorType] of preferredVerSelectorsArr) {
    const { selectorType, weight } = preferredSelectorInfo(preferredSelectorType)
    if (preferredSelector === versionRange) continue
    for (const version of listVersionsMatchingSelector(meta, preferredSelector, selectorType)) {
      versionsPrioritizer.add(version, weight)
    }
  }
  return versionsPrioritizer.versionsByPriority()
}

function listVersionsMatchingSelector (
  meta: PackageMeta,
  preferredSelector: string,
  selectorType: VersionSelectorType
): string[] {
  switch (selectorType) {
    case 'tag':
      return [meta['dist-tags'][preferredSelector]]
    case 'range':
      return Object.keys(meta.versions).filter((version) => semverSatisfiesLoose(version, preferredSelector))
    case 'version':
      return meta.versions[preferredSelector] ? [preferredSelector] : []
  }
  return []
}

class PreferredVersionsPrioritizer {
  private preferredVersions: Record<string, number> = {}

  add (version: string, weight: number): void {
    if (!this.preferredVersions[version]) {
      this.preferredVersions[version] = weight
    } else {
      this.preferredVersions[version] += weight
    }
  }

  versionsByPriority (): string[][] {
    const versionsByWeight = Object.entries(this.preferredVersions)
      .reduce((acc, [version, weight]) => {
        acc[weight] = acc[weight] ?? []
        acc[weight].push(version)
        return acc
      }, {} as Record<number, string[]>)
    return Object.keys(versionsByWeight)
      .sort((a, b) => parseInt(b, 10) - parseInt(a, 10))
      .map((weight) => versionsByWeight[parseInt(weight, 10)])
  }
}

function nonDeprecatedPick (
  meta: PackageMeta,
  candidates: string[],
  picked: string,
  versionRange: string
): string | null {
  if (!meta.versions[picked]?.deprecated || candidates.length <= 1) return null
  const isNonDeprecated = (version: string): boolean => {
    if (version === picked) return false
    const manifest = meta.versions[version]
    return manifest != null && !manifest.deprecated
  }
  if (versionRange === '*' && !semverSatisfiesLoose(picked, versionRange)) {
    const sameRelease = pickSameReleaseVersion(candidates, picked, isNonDeprecated)
    if (sameRelease != null) return sameRelease
  }
  // Filter by the range before touching manifests, so a lazily-loaded
  // packument hydrates only the actual candidates instead of every version.
  const nonDeprecatedVersions = candidates.filter((version) =>
    semverSatisfiesLoose(version, versionRange) && isNonDeprecated(version)
  )
  return maxSatisfyingLoose(nonDeprecatedVersions, versionRange)
}

function pickSameReleaseVersion (
  candidates: string[],
  picked: string,
  isNonDeprecated: (version: string) => boolean
): string | null {
  const pickedParsed = parseSemverLoose(picked)
  if (pickedParsed == null) return null
  return maxVersionLoose(candidates.filter((version) =>
    isSameRelease(parseSemverLoose(version), pickedParsed) && isNonDeprecated(version)
  ))
}

function isSameRelease (parsed: semver.SemVer | null, other: semver.SemVer): boolean {
  return parsed != null &&
    parsed.major === other.major &&
    parsed.minor === other.minor &&
    parsed.patch === other.patch
}
