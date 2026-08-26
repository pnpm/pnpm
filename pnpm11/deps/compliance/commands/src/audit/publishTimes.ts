import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { type AuditAdvisory, satisfiesSafe } from '@pnpm/deps.compliance.audit'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import semver from 'semver'

import type { AuditOptions } from './audit.js'
import { createAuditNetworkOptions } from './auditContext.js'

export interface PackumentPublishInfo {
  /**
   * The packument `time` map: version → raw publish timestamp. Includes the
   * `created` and `modified` metadata keys alongside version keys.
   */
  time: Record<string, string>
  /**
   * Versions the packument marks as deprecated. Deprecated versions are
   * excluded from patched-version validation — a deprecated release is not a
   * viable fix even though it exists on the registry. Normalized rather than
   * kept as raw keys, because the `time` and `versions` maps may spell the
   * same release differently (`v1.2.3` vs `1.2.3`).
   */
  deprecated: ReadonlySet<string>
}

export type PublishTimesFetcher = (pkgName: string) => Promise<PackumentPublishInfo | undefined>

/**
 * Returns a memoized lookup of each package's {@link PackumentPublishInfo}, so
 * callers can tell which versions exist and whether one is young enough that
 * `minimumReleaseAge` would block it. `undefined` means the packument is
 * unknown — the request failed or it carries no `time` field; callers must
 * treat that as "no information", not as "old".
 */
export function createPublishTimesFetcher (opts: AuditOptions): PublishTimesFetcher {
  const networkOptions = createAuditNetworkOptions(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri)
  const fetchFromRegistry = createFetchFromRegistry({
    ca: networkOptions.ca,
    cert: networkOptions.cert,
    httpProxy: networkOptions.httpProxy,
    httpsProxy: networkOptions.httpsProxy,
    key: networkOptions.key,
    localAddress: networkOptions.localAddress,
    maxSockets: networkOptions.maxSockets,
    noProxy: networkOptions.noProxy,
    strictSsl: networkOptions.strictSsl,
    configByUri: opts.configByUri,
  })
  const timesByPkg = new Map<string, Promise<PackumentPublishInfo | undefined>>()
  return (pkgName) => {
    let times = timesByPkg.get(pkgName)
    if (times == null) {
      times = fetchTimes(pkgName)
      timesByPkg.set(pkgName, times)
    }
    return times
  }

  async function fetchTimes (pkgName: string): Promise<PackumentPublishInfo | undefined> {
    try {
      const registry = pickRegistryForPackage(opts.registriesByScope, pkgName)
      const packageUrl = new URL(npa(pkgName).escapedName, registry.endsWith('/') ? registry : `${registry}/`).href
      // Full metadata: the abbreviated packument has no `time` field.
      const res = await fetchFromRegistry(packageUrl, {
        authHeaderValue: getAuthHeader(registry, { pkgName }),
        fullMetadata: true,
        retry: networkOptions.retry,
        timeout: networkOptions.fetchTimeout,
      })
      if (!res.ok) return undefined
      const body = await res.json() as {
        time?: Record<string, string>
        versions?: Record<string, { deprecated?: string }>
      }
      if (body.time == null || typeof body.time !== 'object' || Array.isArray(body.time)) return undefined
      const deprecated = new Set<string>()
      if (body.versions != null && typeof body.versions === 'object') {
        for (const [version, manifest] of Object.entries(body.versions)) {
          if (manifest != null && typeof manifest === 'object' && typeof manifest.deprecated === 'string') {
            const parsed = semver.parse(version, { loose: true })
            if (parsed != null) deprecated.add(parsed.version)
          }
        }
      }
      return { time: body.time as Record<string, string>, deprecated }
    } catch {
      // A failed lookup must not break the fix flow: the caller keeps its
      // current behavior (the exclude entry) when the age is unknown.
      return undefined
    }
  }
}

export interface PublishedVersion {
  /**
   * The `time` key the version was found under. The registry may spell it in
   * a non-normalized form (e.g. `v1.2.3`) that the parsed version drops, so
   * the key is what a publish-time lookup must use.
   */
  key: string
  /**
   * The normalized version.
   */
  version: string
}

/**
 * Returns the lowest non-deprecated published version satisfying `range` — the
 * version an inferred patched range actually resolves to — or `undefined` when
 * no published version satisfies it, whether it was never published, skipped,
 * yanked, or deprecated.
 *
 * Stable releases outrank prereleases regardless of order, so a
 * `4.18.0-beta.1` published before `4.18.0` is never advertised as the fix.
 * A prerelease still wins when nothing else satisfies the range.
 */
export function lowestNonDeprecatedVersion (
  publishInfo: PackumentPublishInfo,
  range: string
): PublishedVersion | undefined {
  let lowest: { key: string, parsed: semver.SemVer } | undefined
  for (const key of Object.keys(publishInfo.time)) {
    if (key === 'created' || key === 'modified') continue
    const parsed = semver.parse(key, { loose: true })
    if (parsed == null || publishInfo.deprecated.has(parsed.version)) continue
    if (!satisfiesSafe(parsed.version, range)) continue
    if (lowest == null || compareFixCandidates(parsed, lowest.parsed) < 0) {
      lowest = { key, parsed }
    }
  }
  return lowest && { key: lowest.key, version: lowest.parsed.version }
}

function compareFixCandidates (a: semver.SemVer, b: semver.SemVer): number {
  const aIsPrerelease = a.prerelease.length > 0
  const bIsPrerelease = b.prerelease.length > 0
  if (aIsPrerelease !== bIsPrerelease) return aIsPrerelease ? 1 : -1
  return semver.compare(a, b)
}

/**
 * Corrects inferred `patched_versions` ranges against the registry: the
 * inference from `vulnerable_versions` is purely syntactic, so the inferred
 * minimum may not be a viable fix — it may never have been published, been
 * skipped, been yanked, or been deprecated. When the inferred range is
 * satisfiable, it is narrowed to the lowest non-deprecated published version
 * (e.g. `>=4.17.24` becomes `>=4.18.1` when 4.17.24 does not exist and
 * 4.18.0 is deprecated). When no published version satisfies it, the range
 * is dropped entirely. A failed packument lookup leaves the range untouched
 * (fail open).
 */
export async function correctInferredPatchedVersions (
  advisories: Record<string, AuditAdvisory>,
  getPublishTimes: PublishTimesFetcher
): Promise<void> {
  await Promise.all(Object.values(advisories).map(async (advisory) => {
    const patched = advisory.patched_versions
    if (patched == null) return
    const publishInfo = await getPublishTimes(advisory.module_name)
    if (publishInfo == null) return
    const lowest = lowestNonDeprecatedVersion(publishInfo, patched)
    if (lowest == null) {
      advisory.patched_versions = null
      advisory.patched_versions_unpublished = true
    } else {
      advisory.patched_versions = `>=${lowest.version}`
    }
  }))
}
