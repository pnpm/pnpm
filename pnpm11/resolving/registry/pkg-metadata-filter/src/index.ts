import { globalWarn } from '@pnpm/logger'
import type { PackageMeta, PackageMetadataWithTime } from '@pnpm/resolving.registry.types'
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
  return filterPkgMetadataVersions(pkgDoc, (version) => {
    const timeStr = pkgDoc.time[version]
    return Boolean(timeStr && new Date(timeStr) <= publishedBy) || trustedVersions?.includes(version) === true
  })
}

/**
 * Returns the packument narrowed to the versions `keep` accepts, with each
 * dist-tag whose version was dropped moved to the best version still kept.
 *
 * The returned document's version manifests are the very objects held by
 * `pkgDoc`.
 */
export function filterPkgMetadataVersions<PkgDoc extends PackageMeta> (
  pkgDoc: PkgDoc,
  keep: (version: string) => boolean
): PkgDoc {
  // Null-prototype so a registry-controlled version like `__proto__` becomes
  // an own key instead of reassigning the map's prototype
  // (js/prototype-polluting-assignment), and so a lookup of an inherited
  // member name can't be mistaken for a version that was kept.
  const keptVersions: PackageMeta['versions'] = Object.create(null)
  for (const version in pkgDoc.versions) {
    if (!Object.hasOwn(pkgDoc.versions, version)) continue
    if (keep(version)) {
      keptVersions[version] = pkgDoc.versions[version]
    }
  }

  const keptDistTags: PackageMeta['dist-tags'] = Object.create(null)
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
    if (keptVersions[distTagVersion]) {
      keptDistTags[tag] = distTagVersion
      continue
    }
    // Repopulate the tag to the best version still kept
    const originalSemVer = tryParseSemver(distTagVersion)
    if (!originalSemVer) continue
    let bestVersion: string | undefined
    let bestParsed: semver.SemVer | undefined
    let bestTier: TagCandidateTier | undefined
    for (const candidate in keptVersions) {
      if (!Object.hasOwn(keptVersions, candidate)) continue
      const candidateParsed = tryParseSemver(candidate)
      if (!candidateParsed || candidateParsed.compare(originalSemVer) > 0) continue
      const tier = getTagCandidateTier(tag, originalSemVer, candidateParsed)
      if (tier == null) continue
      if (bestVersion == null || bestParsed == null || bestTier == null) {
        bestVersion = candidate
        bestParsed = candidateParsed
        bestTier = tier
        continue
      }
      try {
        const candidateIsDeprecated = pkgDoc.versions[candidate].deprecated != null
        const bestVersionIsDeprecated = pkgDoc.versions[bestVersion].deprecated != null
        const candidateRanksHigher = tier !== bestTier ? tier > bestTier : candidateParsed.compare(bestParsed) > 0
        if (
          (candidateRanksHigher && (bestVersionIsDeprecated === candidateIsDeprecated)) ||
          (bestVersionIsDeprecated && !candidateIsDeprecated)
        ) {
          bestVersion = candidate
          bestParsed = candidateParsed
          bestTier = tier
        }
      } catch (_err) {
        globalWarn(`Failed to compare semver versions ${candidate} and ${bestVersion} from packument of ${pkgDoc.name}, skipping candidate version.`)
      }
    }
    if (bestVersion) {
      keptDistTags[tag] = bestVersion
    }
  }

  return {
    ...pkgDoc,
    versions: keptVersions,
    'dist-tags': keptDistTags,
  }
}

/**
 * How well a kept version stands in for a dropped dist-tag target, worst
 * first. The tier is compared before the version.
 */
const TagCandidateTier = {
  /** Same prerelease-ness as the target, on a lower major (`latest` only). */
  LowerMajor: 0,
  /** A prerelease of the stable target's major, below the target. */
  PrereleaseOfMajor: 1,
  /** Same prerelease-ness as the target, on its major. */
  SameLane: 2,
} as const

type TagCandidateTier = typeof TagCandidateTier[keyof typeof TagCandidateTier]

/**
 * A tag keeps its own major and prerelease-ness, except that `latest` may
 * move to a lower major, and a tag that pointed at a stable version may move to a
 * prerelease of its major. That prerelease ranks above any stable version
 * of a lower major: when `1.0.0` is too new, `1.0.0-beta.4` is what
 * `latest` named before it, not `0.0.1`.
 */
function getTagCandidateTier (tag: string, original: semver.SemVer, candidate: semver.SemVer): TagCandidateTier | undefined {
  const originalIsPrerelease = original.prerelease.length > 0
  const candidateIsPrerelease = candidate.prerelease.length > 0
  if (candidateIsPrerelease === originalIsPrerelease) {
    if (candidate.major === original.major) return TagCandidateTier.SameLane
    if (tag !== 'latest') return undefined
    return TagCandidateTier.LowerMajor
  }
  if (candidateIsPrerelease && candidate.major === original.major && candidate.compare(original) < 0) {
    return TagCandidateTier.PrereleaseOfMajor
  }
  return undefined
}
