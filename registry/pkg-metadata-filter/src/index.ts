import { globalWarn } from '@pnpm/logger'
import { type PackageMetadataWithTime } from '@pnpm/registry.types'
import semver from 'semver'

export function filterPkgMetadataByPublishDate (
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
        (tag !== 'latest' && candidateParsed.major !== originalSemVer.major) ||
        (candidateParsed.prerelease.length > 0) !== originalIsPrerelease
      ) continue
      if (!bestVersion) {
        bestVersion = candidate
      } else {
        try {
          const candidateIsDeprecated = pkgDoc.versions[candidate].deprecated != null
          const bestVersionIsDeprecated = pkgDoc.versions[bestVersion].deprecated != null
          if (
            (semver.gt(candidate, bestVersion, true) && (bestVersionIsDeprecated === candidateIsDeprecated)) ||
            (bestVersionIsDeprecated && !candidateIsDeprecated)
          ) {
            bestVersion = candidate
          }
        } catch (err) {
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
