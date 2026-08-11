import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { type AuditAdvisory, satisfiesSafe } from '@pnpm/deps.compliance.audit'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'

import type { AuditOptions } from './audit.js'
import { createAuditNetworkOptions } from './auditContext.js'

/**
 * Returns a memoized lookup of each package's publish-time map (the `time`
 * field of its packument), so callers can tell whether a version is young
 * enough that `minimumReleaseAge` would block it. `undefined` means the
 * publish times are unknown — the request failed or the packument carries no
 * `time` field; callers must treat that as "no information", not as "old".
 */
export function createPublishTimesFetcher (opts: AuditOptions): (pkgName: string) => Promise<Record<string, string> | undefined> {
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
  const timesByPkg = new Map<string, Promise<Record<string, string> | undefined>>()
  return (pkgName) => {
    let times = timesByPkg.get(pkgName)
    if (times == null) {
      times = fetchTimes(pkgName)
      timesByPkg.set(pkgName, times)
    }
    return times
  }

  async function fetchTimes (pkgName: string): Promise<Record<string, string> | undefined> {
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
      const body = await res.json() as { time?: Record<string, string> }
      return body.time != null && typeof body.time === 'object' && !Array.isArray(body.time) ? body.time as Record<string, string> : undefined
    } catch {
      // A failed lookup must not break the fix flow: the caller keeps its
      // current behavior (the exclude entry) when the age is unknown.
      return undefined
    }
  }
}

/**
 * Drops inferred `patched_versions` ranges that no published version
 * satisfies: the inference from `vulnerable_versions` is purely syntactic,
 * so an advisory can claim a patch (e.g. `>=2.0.3`) that was never released.
 * A failed packument lookup leaves the range untouched (fail open). The
 * publish-time map's keys double as the published version list; `created`
 * and `modified` are metadata, not versions.
 */
export async function dropUnsatisfiablePatchedVersions (
  advisories: Record<string, AuditAdvisory>,
  getPublishTimes: (pkgName: string) => Promise<Record<string, string> | undefined>
): Promise<void> {
  await Promise.all(Object.values(advisories).map(async (advisory) => {
    const patched = advisory.patched_versions
    if (patched == null) return
    const times = await getPublishTimes(advisory.module_name)
    if (times == null) return
    const fixPublished = Object.keys(times)
      .filter((key) => key !== 'created' && key !== 'modified')
      .some((version) => satisfiesSafe(version, patched))
    if (!fixPublished) {
      advisory.patched_versions = undefined
      advisory.patched_versions_unpublished = true
    }
  }))
}
