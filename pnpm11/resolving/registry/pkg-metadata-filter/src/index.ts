import { globalWarn } from '@pnpm/logger'
import type { PackageMetadataWithTime } from '@pnpm/resolving.registry.types'
import semver from 'semver'

/**
 * A pick runs for every dependency edge, so the same packument is filtered
 * against the same cutoff many times per install. Filtering allocates a new
 * versions map and parses a Date per version, so memoize per packument
 * object; the key carries the cutoff and trusted versions because a shared
 * meta cache can serve one packument to installs with different policies.
 *
 * A single install computes one cutoff, so one entry per packument covers
 * it. The per-packument map is still capped: a long-lived process (store
 * server, daemon) computes a fresh cutoff per install while a shared meta
 * cache keeps the packument alive, which would otherwise grow the map and
 * retain a filtered copy per install indefinitely.
 */
const MAX_POLICIES_PER_PACKUMENT = 4
const filteredMetaCache = new WeakMap<PackageMetadataWithTime, Map<string, PackageMetadataWithTime>>()

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
  const versionsWithinDate: PackageMetadataWithTime['versions'] = {}
  for (const version in pkgDoc.versions) {
    if (!Object.hasOwn(pkgDoc.versions, version)) continue
    const timeStr = pkgDoc.time[version]
    if ((timeStr && new Date(timeStr) <= publishedBy) || trustedVersions?.includes(version)) {
      versionsWithinDate[version] = pkgDoc.versions[version]
    }
  }

  const distTagsWithinDate: PackageMetadataWithTime['dist-tags'] = {}
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
    for (const candidate in versionsWithinDate) {
      if (!Object.hasOwn(versionsWithinDate, candidate)) continue
      const candidateParsed = tryParseSemver(candidate)
      if (
        !candidateParsed ||
        (tag === 'latest' && candidateParsed.compare(originalSemVer) > 0) ||
        (tag !== 'latest' && candidateParsed.major !== originalSemVer.major) ||
        (candidateParsed.prerelease.length > 0) !== originalIsPrerelease
      ) continue
      if (!bestVersion) {
        bestVersion = candidate
      } else {
        try {
          const candidateIsDeprecated = pkgDoc.versions[candidate].deprecated != null
          const bestVersionIsDeprecated = pkgDoc.versions[bestVersion].deprecated != null
          // Compare the already-parsed instances: bestVersion always parsed
          // successfully when it became the best candidate, and reusing the
          // cached SemVer avoids semver.gt re-parsing both strings.
          const bestParsed = tryParseSemver(bestVersion)
          if (
            bestParsed != null && (
              (candidateParsed.compare(bestParsed) > 0 && (bestVersionIsDeprecated === candidateIsDeprecated)) ||
              (bestVersionIsDeprecated && !candidateIsDeprecated)
            )
          ) {
            bestVersion = candidate
          }
        } catch (_err) {
          globalWarn(`Failed to compare semver versions ${candidate} and ${bestVersion} from packument of ${pkgDoc.name}, skipping candidate version.`)
        }
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
