import { globalWarn } from '@pnpm/logger'
import { type PackageMeta, type PackageMetaWithTime } from '@pnpm/registry.types'
import semver from 'semver'

export function filterMetaByPublishedDate (meta: PackageMetaWithTime, publishedBy: Date): PackageMeta {
  const versionsWithinDate: PackageMeta['versions'] = {}
  for (const version in meta.versions) {
    if (!Object.hasOwn(meta.versions, version)) continue
    const timeStr = meta.time[version]
    if (timeStr && new Date(timeStr) <= publishedBy) {
      versionsWithinDate[version] = meta.versions[version]
    }
  }

  const distTagsWithinDate: PackageMeta['dist-tags'] = {}
  const allDistTags = meta['dist-tags'] ?? {}
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
    // Repopulate the tag to the highest version available within date that has the same major as the original tag's version
    const originalSemVer = tryParseSemver(distTagVersion)
    if (!originalSemVer) continue
    const originalIsPrerelease = (originalSemVer.prerelease.length > 0)
    let bestVersion: string | undefined
    for (const candidate in versionsWithinDate) {
      if (!Object.hasOwn(versionsWithinDate, candidate)) continue
      const candidateParsed = tryParseSemver(candidate)
      if (
        !candidateParsed ||
        candidateParsed.major !== originalSemVer.major ||
        (candidateParsed.prerelease.length > 0) !== originalIsPrerelease
      ) continue
      if (!bestVersion) {
        bestVersion = candidate
      } else {
        try {
          const candidateIsDeprecated = meta.versions[candidate].deprecated != null
          const bestVersionIsDeprecated = meta.versions[bestVersion].deprecated != null
          if (
            (semver.gt(candidate, bestVersion, true) && (bestVersionIsDeprecated === candidateIsDeprecated)) ||
            (bestVersionIsDeprecated && !candidateIsDeprecated)
          ) {
            bestVersion = candidate
          }
        } catch (err) {
          globalWarn(`Failed to compare semver versions ${candidate} and ${bestVersion} from packument of ${meta.name}, skipping candidate version.`)
        }
      }
    }
    if (bestVersion) {
      distTagsWithinDate[tag] = bestVersion
    }
  }

  return {
    ...meta,
    versions: versionsWithinDate,
    'dist-tags': distTagsWithinDate,
  }
}
