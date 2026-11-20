import semver from 'semver'

export function semverSatisfiesLoose (version: string, range: string): boolean {
  const semverRange = parseRangeLoose(range)
  if (semverRange == null) return false
  const parsedVersion = parseSemverLoose(version)
  return parsedVersion != null && semverRange.test(parsedVersion)
}

// semver's own maxSatisfying/minSatisfying re-parse the range and every
// version string on each call, which dominates resolution time on large
// packuments; these reuse the parse caches instead.
/** The newest version by semver order, without a range check. */
export function maxVersionLoose (versions: string[]): string | null {
  let bestVersion: string | null = null
  let bestParsed: semver.SemVer | null = null
  for (const version of versions) {
    const parsed = parseSemverLoose(version)
    if (parsed == null) continue
    if (bestParsed == null || parsed.compare(bestParsed) > 0) {
      bestVersion = version
      bestParsed = parsed
    }
  }
  return bestVersion
}

export function maxSatisfyingLoose (versions: string[], range: string): string | null {
  return findSatisfyingLoose(versions, range, (candidate, best) => candidate.compare(best) > 0)
}

export function minSatisfyingLoose (versions: string[], range: string): string | null {
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

export function parseSemverLoose (version: string): semver.SemVer | null {
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
