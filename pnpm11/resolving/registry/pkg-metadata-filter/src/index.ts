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
  const keptVersions = filterKeptVersions(pkgDoc.versions, keep)
  const keptDistTags = resolveKeptDistTags(pkgDoc, keptVersions)

  return {
    ...pkgDoc,
    versions: keptVersions,
    'dist-tags': keptDistTags,
  }
}

function filterKeptVersions (
  versions: PackageMeta['versions'],
  keep: (version: string) => boolean
): PackageMeta['versions'] {
  const keptVersions: PackageMeta['versions'] = Object.create(null)
  for (const version in versions) {
    if (!Object.hasOwn(versions, version)) continue
    if (keep(version)) {
      // Copy the property descriptor, not the value: a lazily-loaded packument
      // (see npm-resolver's mirrorLayout module) backs each version with a
      // getter that parses its manifest on first access, and reading the value
      // here would parse every kept version of every packument up front.
      Object.defineProperty(keptVersions, version, Object.getOwnPropertyDescriptor(versions, version)!)
    }
  }
  return keptVersions
}

interface BestCandidate {
  version: string
  parsed: semver.SemVer
  tier: TagCandidateTier
}

function resolveKeptDistTags (
  pkgDoc: PackageMeta,
  keptVersions: PackageMeta['versions']
): PackageMeta['dist-tags'] {
  const keptDistTags: PackageMeta['dist-tags'] = Object.create(null)
  const allDistTags = pkgDoc['dist-tags'] ?? {}
  const semverCache = new Map<string, semver.SemVer>()
  const parse = (str: string) => tryParseSemver(str, semverCache)

  for (const tag in allDistTags) {
    if (!Object.hasOwn(allDistTags, tag)) continue
    const distTagVersion = allDistTags[tag]
    // `in`, not a value read: presence is all that matters, and reading the
    // value would hydrate a lazily-loaded manifest for nothing.
    if (distTagVersion in keptVersions) {
      keptDistTags[tag] = distTagVersion
      continue
    }
    const best = findBestTagCandidate({
      distTagVersion,
      keptVersions,
      parse,
      pkgDoc,
      tag,
    })
    if (best) {
      keptDistTags[tag] = best.version
    }
  }
  return keptDistTags
}

function tryParseSemver (semverStr: string, cache: Map<string, semver.SemVer>): semver.SemVer | null {
  let parsed = cache.get(semverStr)
  if (!parsed) {
    try {
      parsed = new semver.SemVer(semverStr, true)
    } catch {
      return null
    }
    cache.set(semverStr, parsed)
  }
  return parsed
}

interface FindBestTagCandidateOptions {
  distTagVersion: string
  keptVersions: PackageMeta['versions']
  parse: (str: string) => semver.SemVer | null
  pkgDoc: PackageMeta
  tag: string
}

function findBestTagCandidate (opts: FindBestTagCandidateOptions): BestCandidate | undefined {
  const { distTagVersion, keptVersions, parse, pkgDoc, tag } = opts
  const originalSemVer = parse(distTagVersion)
  if (!originalSemVer) return undefined
  let best: BestCandidate | undefined

  for (const candidate in keptVersions) {
    if (!Object.hasOwn(keptVersions, candidate)) continue
    const candidateInfo = evaluateTagCandidate({
      candidate,
      originalSemVer,
      parse,
      tag,
    })
    if (candidateInfo == null) continue
    if (best == null || candidateIsBetter(candidateInfo, best, pkgDoc)) {
      best = candidateInfo
    }
  }
  return best
}

interface EvaluateTagCandidateOptions {
  candidate: string
  originalSemVer: semver.SemVer
  parse: (str: string) => semver.SemVer | null
  tag: string
}

function evaluateTagCandidate (opts: EvaluateTagCandidateOptions): BestCandidate | undefined {
  const { candidate, originalSemVer, parse, tag } = opts
  const candidateParsed = parse(candidate)
  if (!candidateParsed || candidateParsed.compare(originalSemVer) > 0) return undefined
  const tier = getTagCandidateTier(tag, originalSemVer, candidateParsed)
  if (tier == null) return undefined
  return { version: candidate, parsed: candidateParsed, tier }
}


function candidateIsBetter (
  candidate: BestCandidate,
  best: BestCandidate,
  pkgDoc: PackageMeta
): boolean {
  try {
    const candidateIsDeprecated = pkgDoc.versions[candidate.version].deprecated != null
    const bestVersionIsDeprecated = pkgDoc.versions[best.version].deprecated != null
    const ranksHigher = candidate.tier !== best.tier
      ? candidate.tier > best.tier
      : candidate.parsed.compare(best.parsed) > 0
    return (
      (ranksHigher && (bestVersionIsDeprecated === candidateIsDeprecated)) ||
      (bestVersionIsDeprecated && !candidateIsDeprecated)
    )
  } catch (_err) {
    globalWarn(`Failed to compare semver versions ${candidate.version} and ${best.version} from packument of ${pkgDoc.name}, skipping candidate version.`)
    return false
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
