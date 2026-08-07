import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import type { Registries } from '@pnpm/types'

import type { AuditOptions } from './audit.js'
import { createAuditNetworkOptions } from './auditContext.js'

/**
 * Returns the list of versions published for a package, or `undefined` when
 * that cannot be determined (registry unreachable, unexpected body, ...).
 * `undefined` means "unknown", never "nothing is published".
 */
export type GetPublishedVersions = (packageName: string) => Promise<string[] | undefined>

export function createGetPublishedVersions (opts: AuditOptions): GetPublishedVersions {
  const networkOptions = createAuditNetworkOptions(opts)
  const fetchFromRegistry = createFetchFromRegistry({
    ca: networkOptions.ca,
    cert: networkOptions.cert,
    configByUri: networkOptions.configByUri,
    httpProxy: networkOptions.httpProxy,
    httpsProxy: networkOptions.httpsProxy,
    key: networkOptions.key,
    localAddress: networkOptions.localAddress,
    maxSockets: networkOptions.maxSockets,
    noProxy: networkOptions.noProxy,
    strictSsl: networkOptions.strictSsl,
    timeout: networkOptions.fetchTimeout,
  })
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri)
  const cache = new Map<string, Promise<string[] | undefined>>()
  return async (packageName: string): Promise<string[] | undefined> => {
    let versions = cache.get(packageName)
    if (versions == null) {
      versions = fetchPublishedVersions(packageName, {
        fetchFromRegistry,
        getAuthHeader,
        registries: opts.registries,
        retry: networkOptions.retry,
        timeout: networkOptions.fetchTimeout,
      })
      cache.set(packageName, versions)
    }
    return versions
  }
}

async function fetchPublishedVersions (
  packageName: string,
  opts: {
    fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
    getAuthHeader: GetAuthHeader
    registries: Registries
    retry?: { factor?: number, maxTimeout?: number, minTimeout?: number, retries?: number }
    timeout?: number
  }
): Promise<string[] | undefined> {
  const registry = pickRegistryForPackage(opts.registries, packageName)
  // Scoped names are escaped the way registries expect them, with the leading
  // "@" left as is, so "@scope/name" becomes "@scope%2Fname".
  const escapedName = encodeURIComponent(packageName).replace(/^%40/, '@')
  const url = `${registry.replace(/\/$/, '')}/${escapedName}`
  try {
    const res = await opts.fetchFromRegistry(url, {
      authHeaderValue: opts.getAuthHeader(registry, { pkgName: packageName }),
      retry: opts.retry,
      timeout: opts.timeout,
    })
    if (res.status !== 200) return undefined
    const body: unknown = await res.json()
    if (typeof body !== 'object' || body === null) return undefined
    const { versions } = body as { versions?: unknown }
    if (typeof versions !== 'object' || versions === null || Array.isArray(versions)) return undefined
    return Object.keys(versions)
  } catch {
    // The registry may be private, offline or behind a proxy that doesn't
    // serve packuments. Treat that as "unknown" so the fix isn't blocked.
    return undefined
  }
}
