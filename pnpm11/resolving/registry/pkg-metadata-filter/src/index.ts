import { globalWarn } from '@pnpm/logger'
import type { PackageMetadataWithTime } from '@pnpm/resolving.registry.types'
import semver from 'semver'

/**
 * A pick runs for every dependency edge, so the same packument is filtered
 * against the same cutoff many times per install. Filtering allocates a new
 * versions map and parses a Date per version, so the result is memoized per
 * packument object; the key carries the cutoff and the trusted versions
 * because a shared meta cache can serve one packument to installs with
 * different policies.
 *
 * A single install computes one cutoff, so one entry per packument covers it.
 * The per-packument map is capped anyway: a long-lived process (store server,
 * daemon) computes a fresh cutoff per install while a shared meta cache keeps
 * the packument alive, which would otherwise retain a filtered copy per
 * install indefinitely.
 */
const MAX_POLICIES_PER_PACKUMENT = 4
const filteredMetaCache = new WeakMap<PackageMetadataWithTime, Map<string, PackageMetadataWithTime>>()

/**
 * Returns the packument narrowed to the versions published at or before
 * `publishedBy`, plus any `trustedVersions`, with each dist-tag moved to the
 * best version still within date.
 *
 * The returned document is shared between callers and must be treated as
 * read-only; its version manifests are the very objects held by `pkgDoc`.
 */
export function filterPkgMetadataByPublishDate (
  pkgDoc: PackageMetadataWithTime,
  publishedBy: Date,
  trustedVersions?: string[]
): PackageMetadataWithTime {
  let byPolicy = filteredMetaCache.get(pkgDoc)
  if (byPolicy == null) {
    byPolicy = new Map()
    filteredMetaCache.set(pkgDoc, byPolicy)
  }
  const policyKey = trustedVersions == null
    ? String(publishedBy.getTime())
    : `${publishedBy.getTime()}\x00${trustedVersions.join('\x00')}`
  let filtered = byPolicy.get(policyKey)
  if (filtered == null) {
    filtered = filterPkgMetadataByPublishDateUncached(pkgDoc, publishedBy, trustedVersions)
    if (byPolicy.size >= MAX_POLICIES_PER_PACKUMENT) {
      // Map preserves insertion order, so the first key is the oldest policy.
      byPolicy.delete(byPolicy.keys().next().value!)
    }
    byPolicy.set(policyKey, filtered)
  }
  return filtered
}

function filterPkgMetadataByPublishDateUncached (
  pkgDoc: PackageMetadataWithTime,
  publishedBy: Date,
  trustedVersions?: string[]
): PackageMetadataWithTime {
  // Null-prototype so a registry-controlled version like `__proto__` becomes
  // an own key instead of reassigning the map's prototype
  // (js/prototype-polluting-assignment), and so a lookup of an inherited
  // member name can't be mistaken for a version that is within date.
  const versionsWithinDate: PackageMetadataWithTime['versions'] = Object.create(null)
  for (const version in pkgDoc.versions) {
    if (!Object.hasOwn(pkgDoc.versions, version)) continue
    const timeStr = pkgDoc.time[version]
    if ((timeStr && new Date(timeStr) <= publishedBy) || trustedVersions?.includes(version)) {
      versionsWithinDate[version] = pkgDoc.versions[version]
    }
  }

  const distTagsWithinDate: PackageMetadataWithTime['dist-tags'] = Object.create(null)
  const allDistTags = pkgDoc['dist-tags'] ?? {}
  const parsedSemverCache = new Map<string, semver.SemVer>()
  function tryParseSemver (semverStr: string): semver.SemVer | null {
    let parsedSemver = parsedSemverCache.get(semverStr)
    if (!parsedSemver) {
      try {
        parsedSemver = new semver.SemVer(semverStr, true)
      } catch {
        return null
      }
      parsedSemverCache.set(semverStr, parsedSemver)
    }
    return parsedSemver
  }
  for (const tag in allDistTags) {
    if (!Object.hasOwn(allDistTags, tag)) continue
    const distTagVersion = allDistTags[tag]
    if (versionsWithinDate[distTagVersion]) {
      distTagsWithinDate[tag] = distTagVersion
      continue
    }
    // Repopulate the tag to the highest version available within date
    const originalSemVer = tryParseSemver(distTagVersion)
    if (!originalSemVer) continue
    const originalIsPrerelease = (originalSemVer.prerelease.length > 0)
    let bestVersion: string | undefined
    let bestParsed: semver.SemVer | undefined
    for (const candidate in versionsWithinDate) {
      if (!Object.hasOwn(versionsWithinDate, candidate)) continue
      const candidateParsed = tryParseSemver(candidate)
      if (
        !candidateParsed ||
        candidateParsed.compare(originalSemVer) > 0 ||
        (tag !== 'latest' && candidateParsed.major !== originalSemVer.major) ||
        (candidateParsed.prerelease.length > 0) !== originalIsPrerelease
      ) continue
      if (bestVersion == null || bestParsed == null) {
        bestVersion = candidate
        bestParsed = candidateParsed
        continue
      }
      try {
        const candidateIsDeprecated = pkgDoc.versions[candidate].deprecated != null
        const bestVersionIsDeprecated = pkgDoc.versions[bestVersion].deprecated != null
        if (
          (candidateParsed.compare(bestParsed) > 0 && (bestVersionIsDeprecated === candidateIsDeprecated)) ||
          (bestVersionIsDeprecated && !candidateIsDeprecated)
        ) {
          bestVersion = candidate
          bestParsed = candidateParsed
        }
      } catch (_err) {
        globalWarn(`Failed to compare semver versions ${candidate} and ${bestVersion} from packument of ${pkgDoc.name}, skipping candidate version.`)
      }
    }
    if (bestVersion) {
      distTagsWithinDate[tag] = bestVersion
    }
  }

  return {
    ...pkgDoc,
    versions: versionsWithinDate,
    'dist-tags': distTagsWithinDate,
  }
}
