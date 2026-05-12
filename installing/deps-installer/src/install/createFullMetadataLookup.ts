import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { fetchMetadataFromFromRegistry, type PackageMeta } from '@pnpm/resolving.npm-resolver'
import type { Registries, RegistryConfig } from '@pnpm/types'

import type { ManifestLookup, ManifestLookupResult } from './revalidateLockfileMinimumReleaseAge.js'

export interface CreateFullMetadataLookupOptions {
  registries: Registries
  configByUri?: Record<string, RegistryConfig>
  userAgent?: string
  retry?: Parameters<typeof fetchMetadataFromFromRegistry>[0]['retry']
  timeout?: number
}

/**
 * Build a `ManifestLookup` that goes directly to the registry asking for the
 * full (un-abbreviated) metadata document. Used by the lockfile revalidation
 * gate so that `meta.time[version]` is reliably available — abbreviated
 * metadata, store peek fast paths, and resolver-side `minimumReleaseAge`
 * filtering would all hide the publish timestamp we need to inspect.
 *
 * This mirrors `populateManifestCache(.all)` in bun: a deliberate full-manifest
 * fetch decoupled from the regular resolver pipeline (oven-sh/bun#30526).
 */
export function createFullMetadataLookup (opts: CreateFullMetadataLookupOptions): ManifestLookup {
  const fetchFromRegistry = createFetchFromRegistry({
    userAgent: opts.userAgent,
    configByUri: opts.configByUri,
  })
  const getAuthHeaderValueByURI = createGetAuthHeaderByURI(opts.configByUri ?? {}, opts.registries.default)
  const fetchOpts = {
    fetch: fetchFromRegistry,
    retry: opts.retry ?? {},
    timeout: opts.timeout ?? 60_000,
    fetchWarnTimeoutMs: 10_000,
  }
  // Cache identical (registry, pkgName) fetches across the run; the lockfile
  // can pin many versions of the same package and the metadata document is the
  // same for all of them.
  const inflight = new Map<string, Promise<PackageMeta>>()
  const fetchMeta = async (registry: string, name: string): Promise<PackageMeta> => {
    const cacheKey = `${registry}\x00${name}`
    const cached = inflight.get(cacheKey)
    if (cached) return cached
    const promise = (async () => {
      const result = await fetchMetadataFromFromRegistry(fetchOpts, name, {
        registry,
        authHeaderValue: getAuthHeaderValueByURI(registry),
        fullMetadata: true,
      })
      if ('notModified' in result && result.notModified) {
        throw new PnpmError(
          'REVALIDATE_NOT_MODIFIED_WITHOUT_CACHE',
          `Registry returned 304 for ${name} without an existing cache to refresh.`
        )
      }
      return result.meta
    })()
    inflight.set(cacheKey, promise)
    return promise
  }

  return async (name: string, version: string): Promise<ManifestLookupResult> => {
    const registry = pickRegistryForPackage(opts.registries, name)
    let meta: PackageMeta
    try {
      meta = await fetchMeta(registry, name)
    } catch (err) {
      return {
        status: 'manifest-unavailable',
        reason: err instanceof Error ? err.message : String(err),
      }
    }
    const time = meta.time?.[version]
    if (!time) {
      // For full metadata this means the version was removed from the manifest
      // (typically a deliberate unpublish) or the registry doesn't expose
      // per-version timestamps for it. Either way the release-age cannot be
      // verified, so report it as a violation rather than silently passing.
      return { status: 'version-not-in-manifest' }
    }
    return { status: 'ok', publishedAt: new Date(time) }
  }
}
