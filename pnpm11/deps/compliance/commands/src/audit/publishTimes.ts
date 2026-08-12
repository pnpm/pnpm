import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { type AuditAdvisory, satisfiesSafe } from '@pnpm/deps.compliance.audit'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'

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
   * viable fix even though it exists on the registry.
   */
  deprecated: ReadonlySet<string>
}

export type PublishTimesFetcher = (pkgName: string) => Promise<PackumentPublishInfo | undefined>

/**
 * Returns a memoized lookup of each package's publish-time map (the `time`
 * field of its packument), so callers can tell whether a version is young
 * enough that `minimumReleaseAge` would block it. `undefined` means the
 * publish times are unknown — the request failed or the packument carries no
 * `time` field; callers must treat that as "no information", not as "old".
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
            deprecated.add(version)
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

/**
 * Returns the lowest non-deprecated published version satisfying `range`, or
 * `undefined` when no published version satisfies it (whether it was never
 * published, skipped, yanked, or deprecated). A failed packument lookup
 * (`undefined` publish info) also returns `undefined`.
 */
export function lowestNonDeprecatedVersion (
  publishInfo: PackumentPublishInfo | undefined,
  range: string
): string | undefined {
  if (publishInfo == null) return undefined
  const versions = Object.keys(publishInfo.time)
    .filter((key) => key !== 'created' && key !== 'modified' && !publishInfo.deprecated.has(key))
  let lowest: string | undefined
  for (const version of versions) {
    if (satisfiesSafe(version, range) && (lowest == null || compareVersions(version, lowest) < 0)) {
      lowest = version
    }
  }
  return lowest
}

function compareVersions (a: string, b: string): number {
  const pa = a.split('.').map(Number)
  const pb = b.split('.').map(Number)
  for (let i = 0; i < 3; i++) {
    const diff = (pa[i] || 0) - (pb[i] || 0)
    if (diff !== 0) return diff
  }
  return 0
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
 * (fail open). Ports pnpm's `correctInferredPatchedVersions`.
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
      advisory.patched_versions = `>=${lowest}`
    }
  }))
}
