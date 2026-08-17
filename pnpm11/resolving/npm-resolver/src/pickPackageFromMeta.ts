import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { filterPkgMetadataByPublishDate } from '@pnpm/resolving.registry.pkg-metadata-filter'
import type { PackageInRegistry, PackageMeta, PackageMetaWithTime } from '@pnpm/resolving.registry.types'
import {
  EXISTING_VERSION_SELECTOR_WEIGHT,
  type VersionSelectors,
  type VersionSelectorType,
} from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'

import type { RegistryPackageSpec } from './parseBareSpecifier.js'

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
    const view = applyPublishedByPolicy(meta, publishedBy, publishedByExclude)
    meta = view.meta
    if (view.needsFullMetadata) {
      const modifiedDate = parseModifiedDate(meta.modified)
      if (modifiedDate == null || modifiedDate > publishedBy) {
        // The package was modified after the cutoff (or carries no usable
        // `modified`), so which of its versions are mature is unknowable
        // from abbreviated metadata. The error tells the caller to refetch.
        assertMetaHasTime(meta)
      }
      // else: `modified` is an upper bound on every per-version timestamp, so
      // `modified <= publishedBy` means they all pass the maturity filter and
      // nothing would be dropped. Inclusive at the boundary on purpose, to
      // match the per-version `<=` in `filterPkgMetadataByPublishDate`.
    }
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
    let version!: string | null
    switch (spec.type) {
      case 'version':
        version = spec.fetchSpec
        break
      case 'tag':
        version = meta['dist-tags'][spec.fetchSpec]
        break
      case 'range':
        version = pickVersionByVersionRangeFn({
          meta,
          versionRange: spec.fetchSpec,
          preferredVersionSelectors,
          publishedBy,
        })
        break
    }
    if (!version) return null
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
  } catch (err: unknown) {
    if (
      util.types.isNativeError(err) &&
      'code' in err &&
      typeof err.code === 'string' &&
      err.code.startsWith('ERR_PNPM_')
    ) {
      throw err
    }
    throw new PnpmError('MALFORMED_METADATA',
      `Received malformed metadata for "${spec.name}"`,
      { hint: 'This might mean that the package was unpublished from the registry', cause: err }
    )
  }
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

function parseModifiedDate (modified: string | undefined): Date | null {
  if (!modified) return null
  const date = new Date(modified)
  if (Number.isNaN(date.getTime())) return null
  return date
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

export function pickVersionByVersionRange ({ meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions): string | null {
  const latest: string | undefined = meta['dist-tags'].latest

  if (preferredVersionSelectors != null && Object.keys(preferredVersionSelectors).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
    for (const preferredVersions of prioritizedPreferredVersions) {
      if (preferredVersions.includes(latest) && semverSatisfiesLoose(latest, versionRange)) {
        return latest
      }
      const preferredVersion = maxSatisfyingLoose(preferredVersions, versionRange)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }

  const versions = Object.keys(meta.versions)
  if (latest && (versionRange === '*' || semverSatisfiesLoose(latest, versionRange))) {
    // Not using semver.satisfies in case of * because it does not select beta versions.
    // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
    return latest
  }

  const maxVersion = maxSatisfyingLoose(versions, versionRange)

  // if the selected version is deprecated, try to find a non-deprecated one that satisfies the range
  if (maxVersion && meta.versions[maxVersion].deprecated && versions.length > 1) {
    const nonDeprecatedVersions = versions.map((version) => meta.versions[version])
      .filter((versionMeta) => !versionMeta.deprecated)
      .map((versionMeta) => versionMeta.version)

    const maxNonDeprecatedVersion = maxSatisfyingLoose(nonDeprecatedVersions, versionRange)
    if (maxNonDeprecatedVersion) return maxNonDeprecatedVersion
  }
  return maxVersion
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

export function getDominantLockfileVersion (
  versionRange: string,
  preferredVersionSelectors?: VersionSelectors
): string | null {
  if (preferredVersionSelectors == null) return null
  let lockfileVersion: string | undefined
  for (const [selector, value] of Object.entries(preferredVersionSelectors)) {
    if (selector === versionRange) continue
    const { selectorType, weight } = preferredSelectorInfo(value)
    if (!Number.isSafeInteger(weight) || weight <= 0) return null
    if (
      selectorType === 'version' &&
      weight >= EXISTING_VERSION_SELECTOR_WEIGHT &&
      semverSatisfiesLoose(selector, versionRange)
    ) {
      if (lockfileVersion != null) return null
      lockfileVersion = selector
    }
  }
  if (lockfileVersion == null) return null

  let guaranteedLockfileWeight = 0
  let maximumOtherVersionWeight = 0
  for (const [selector, value] of Object.entries(preferredVersionSelectors)) {
    if (selector === versionRange) continue
    const { selectorType, weight } = preferredSelectorInfo(value)
    switch (selectorType) {
      case 'version':
        if (selector === lockfileVersion) {
          guaranteedLockfileWeight += weight
        } else if (
          weight < EXISTING_VERSION_SELECTOR_WEIGHT &&
          semverSatisfiesLoose(selector, versionRange)
        ) {
          maximumOtherVersionWeight += weight
        }
        break
      case 'range':
        if (semverSatisfiesLoose(lockfileVersion, selector)) {
          guaranteedLockfileWeight += weight
        }
        // Conservatively assume an unseen version can satisfy every preferred
        // range, even when proving range intersection would be more precise.
        maximumOtherVersionWeight += weight
        break
      case 'tag':
        // A registry can move a tag between requests. Do not count its current
        // target toward the lockfile version, and assume all tags could move to
        // the same unseen version.
        maximumOtherVersionWeight += weight
        break
    }
    if (
      !Number.isSafeInteger(guaranteedLockfileWeight) ||
      !Number.isSafeInteger(maximumOtherVersionWeight)
    ) return null
  }
  return guaranteedLockfileWeight > maximumOtherVersionWeight
    ? lockfileVersion
    : null
}

function preferredSelectorInfo (
  value: VersionSelectors[string]
): { selectorType: VersionSelectorType, weight: number } {
  return typeof value === 'string'
    ? { selectorType: value, weight: 1 }
    : value
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
    switch (selectorType) {
      case 'tag': {
        versionsPrioritizer.add(meta['dist-tags'][preferredSelector], weight)
        break
      }
      case 'range': {
        const versions = Object.keys(meta.versions)
        for (const version of versions) {
          if (semverSatisfiesLoose(version, preferredSelector)) {
            versionsPrioritizer.add(version, weight)
          }
        }
        break
      }
      case 'version': {
        if (meta.versions[preferredSelector]) {
          versionsPrioritizer.add(preferredSelector, weight)
        }
        break
      }
    }
  }
  return versionsPrioritizer.versionsByPriority()
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

function semverSatisfiesLoose (version: string, range: string): boolean {
  const semverRange = parseRangeLoose(range)
  if (semverRange == null) return false
  const parsedVersion = parseSemverLoose(version)
  return parsedVersion != null && semverRange.test(parsedVersion)
}

// semver's own maxSatisfying/minSatisfying re-parse the range and every
// version string on each call, which dominates resolution time on large
// packuments; these reuse the parse caches instead.
function maxSatisfyingLoose (versions: string[], range: string): string | null {
  return findSatisfyingLoose(versions, range, (candidate, best) => candidate.compare(best) > 0)
}

function minSatisfyingLoose (versions: string[], range: string): string | null {
  return findSatisfyingLoose(versions, range, (candidate, best) => candidate.compare(best) < 0)
}

function findSatisfyingLoose (
  versions: string[],
  range: string,
  isBetter: (candidate: semver.SemVer, best: semver.SemVer) => boolean
): string | null {
  const semverRange = parseRangeLoose(range)
  if (semverRange == null) return null
  let bestVersion: string | null = null
  let bestParsed: semver.SemVer | null = null
  for (const version of versions) {
    const parsed = parseSemverLoose(version)
    if (parsed == null || !semverRange.test(parsed)) continue
    if (bestParsed == null || isBetter(parsed, bestParsed)) {
      bestVersion = version
      bestParsed = parsed
    }
  }
  return bestVersion
}

function parseRangeLoose (range: string): semver.Range | null {
  let semverRange = semverRangeCache.get(range)
  if (semverRange === undefined) {
    try {
      semverRange = new semver.Range(range, true)
    } catch {
      semverRange = null
    }
    if (semverRangeCache.size >= SEMVER_CACHE_MAX_SIZE) semverRangeCache.clear()
    semverRangeCache.set(range, semverRange)
  }
  return semverRange
}

function parseSemverLoose (version: string): semver.SemVer | null {
  let parsed = semverInstanceCache.get(version)
  if (parsed === undefined) {
    try {
      parsed = new semver.SemVer(version, true)
    } catch {
      parsed = null
    }
    if (semverInstanceCache.size >= SEMVER_CACHE_MAX_SIZE) semverInstanceCache.clear()
    semverInstanceCache.set(version, parsed)
  }
  return parsed
}

// Working with string-ish semver causes lots of allocations and repeated
// work, and a dependency graph tests the same ranges and versions over and
// over, so both parses are cached. Parse failures are cached as null, so a
// malformed version string is never re-parsed either. The caches are dropped
// wholesale once they grow past this size, so a long-lived process (daemon,
// store server) can't retain them without bound.
const SEMVER_CACHE_MAX_SIZE = 50_000
const semverRangeCache = new Map<string, semver.Range | null>()
const semverInstanceCache = new Map<string, semver.SemVer | null>()
