import { globalWarn } from '@pnpm/logger'
import type { PackageMetadata } from '@pnpm/resolving.registry.types'
import semver from 'semver'

/**
 * Which versions of a packument a resolution may pick from.
 *
 * A version survives when it is not blocked and either the cutoff admits it
 * or the policy trusts it by name. `publishedBy` is optional so a package
 * `minimumReleaseAgeExclude` covers wholesale can still have individual
 * versions blocked — the cutoff does not apply to it, but a version whose
 * own dependencies cannot satisfy the cutoff still has to go.
 */
export interface PkgMetadataFilter {
  /** Drop versions published after this instant. Omit to admit every version regardless of age. */
  publishedBy?: Date
  /** Versions to keep even when `publishedBy` would drop them. */
  trustedVersions?: string[]
  /** Versions to drop whatever the cutoff says. */
  blockedVersions?: ReadonlySet<string>
}

/**
 * A pick runs for every dependency edge, so the same packument is filtered
 * against the same policy many times per install. Filtering allocates a new
 * versions map and parses a Date per version, so the result is memoized per
 * packument object; the key carries every field of the filter because a
 * shared meta cache can serve one packument to installs with different
 * policies, and one install can re-resolve under a grown blocklist.
 *
 * A single install computes one cutoff, so a handful of entries per packument
 * covers it. The per-packument map is capped anyway: a long-lived process
 * (store server, daemon) computes a fresh cutoff per install while a shared
 * meta cache keeps the packument alive, which would otherwise retain a
 * filtered copy per install indefinitely.
 */
const MAX_POLICIES_PER_PACKUMENT = 4
const filteredMetaCache = new WeakMap<PackageMetadata, Map<string, PackageMetadata>>()

/**
 * Returns the packument narrowed to the versions {@link PkgMetadataFilter}
 * admits, with each dist-tag moved to the best version still admitted.
 *
 * The returned document is shared between callers and must be treated as
 * read-only; its version manifests are the very objects held by `pkgDoc`.
 */
export function filterPkgMetadata (
  pkgDoc: PackageMetadata,
  filter: PkgMetadataFilter
): PackageMetadata {
  let byPolicy = filteredMetaCache.get(pkgDoc)
  if (byPolicy == null) {
    byPolicy = new Map()
    filteredMetaCache.set(pkgDoc, byPolicy)
  }
  const policyKey = toPolicyKey(filter)
  let filtered = byPolicy.get(policyKey)
  if (filtered == null) {
    filtered = filterPkgMetadataUncached(pkgDoc, filter)
    if (byPolicy.size >= MAX_POLICIES_PER_PACKUMENT) {
      // Map preserves insertion order, so the first key is the oldest policy.
      byPolicy.delete(byPolicy.keys().next().value!)
    }
    byPolicy.set(policyKey, filtered)
  }
  return filtered
}

function toPolicyKey ({ publishedBy, trustedVersions, blockedVersions }: PkgMetadataFilter): string {
  // Serialized rather than joined on a delimiter: a version string is
  // registry-controlled and may contain whatever separator we picked, so
  // `['a\0b']` and `['a', 'b']` would key the same entry and one policy would
  // be served the other's filtered packument. JSON escapes the separators it
  // uses, so distinct collections always produce distinct keys.
  //
  // Both collections are sorted first, so two spellings of one policy do
  // share an entry rather than evicting each other from a cache this small.
  return JSON.stringify([
    publishedBy?.getTime() ?? null,
    trustedVersions == null ? null : [...trustedVersions].sort(),
    blockedVersions == null ? null : [...blockedVersions].sort(),
  ])
}

function filterPkgMetadataUncached (
  pkgDoc: PackageMetadata,
  { publishedBy, trustedVersions, blockedVersions }: PkgMetadataFilter
): PackageMetadata {
  // Null-prototype so a registry-controlled version like `__proto__` becomes
  // an own key instead of reassigning the map's prototype
  // (js/prototype-polluting-assignment), and so a lookup of an inherited
  // member name can't be mistaken for a version that is admitted.
  const admittedVersions: PackageMetadata['versions'] = Object.create(null)
  for (const version in pkgDoc.versions) {
    if (!Object.hasOwn(pkgDoc.versions, version)) continue
    if (blockedVersions?.has(version)) continue
    if (publishedBy == null || trustedVersions?.includes(version)) {
      admittedVersions[version] = pkgDoc.versions[version]
      continue
    }
    const timeStr = pkgDoc.time?.[version]
    if (timeStr && new Date(timeStr) <= publishedBy) {
      admittedVersions[version] = pkgDoc.versions[version]
    }
  }

  const admittedDistTags: PackageMetadata['dist-tags'] = Object.create(null)
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
    if (admittedVersions[distTagVersion]) {
      admittedDistTags[tag] = distTagVersion
      continue
    }
    // Repopulate the tag to the highest version that is still admitted
    const originalSemVer = tryParseSemver(distTagVersion)
    if (!originalSemVer) continue
    const originalIsPrerelease = (originalSemVer.prerelease.length > 0)
    let bestVersion: string | undefined
    let bestParsed: semver.SemVer | undefined
    for (const candidate in admittedVersions) {
      if (!Object.hasOwn(admittedVersions, candidate)) continue
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
      admittedDistTags[tag] = bestVersion
    }
  }

  return {
    ...pkgDoc,
    versions: admittedVersions,
    'dist-tags': admittedDistTags,
  }
}
