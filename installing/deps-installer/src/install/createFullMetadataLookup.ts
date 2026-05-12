import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type DispatcherOptions } from '@pnpm/network.fetch'
import { fetchMetadataFromFromRegistry, type PackageMeta } from '@pnpm/resolving.npm-resolver'
import type { Registries, RegistryConfig } from '@pnpm/types'

import type { ManifestLookup, ManifestLookupResult } from './revalidateLockfileMinimumReleaseAge.js'

export interface CreateFullMetadataLookupOptions extends DispatcherOptions {
  registries: Registries
  /**
   * Registries reached via the named-registry resolver chain (e.g. `gh:` →
   * GitHub Packages). When a lockfile entry's tarball URL lives under one of
   * these, route the manifest fetch to that registry instead of the
   * scope-derived default.
   */
  namedRegistries?: Record<string, string>
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
  // Forward every dispatcher option (proxy / CA / strictSsl / maxSockets /
  // localAddress / etc.) so the revalidation fetcher behaves the same way as
  // the regular store-controller fetch chain. Dropping them would break CI
  // environments behind a proxy or with custom TLS material.
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeaderValueByURI = createGetAuthHeaderByURI(opts.configByUri ?? {}, opts.registries.default)
  const fetchOpts = {
    fetch: fetchFromRegistry,
    retry: opts.retry ?? {},
    timeout: opts.timeout ?? 60_000,
    fetchWarnTimeoutMs: 10_000,
  }
  // Pre-compute the (origin → registry URL) lookup once. Named-registry
  // entries are absolute URLs; their tarballs always live under that origin.
  // We use this to recover the right registry for non-scope-routed packages
  // when the lockfile tells us where the tarball came from.
  const namedRegistryOrigins = new Map<string, string>()
  for (const url of Object.values(opts.namedRegistries ?? {})) {
    try {
      namedRegistryOrigins.set(new URL(url).origin, url)
    } catch {
      // Malformed URL — pickRegistryForPackage will surface its own error if
      // it actually gets used.
    }
  }
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

  return async (name: string, version: string, tarballUrl?: string): Promise<ManifestLookupResult> => {
    const registry = pickRegistryForVersion(opts, namedRegistryOrigins, name, tarballUrl)
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

function pickRegistryForVersion (
  opts: CreateFullMetadataLookupOptions,
  namedRegistryOrigins: Map<string, string>,
  name: string,
  tarballUrl: string | undefined
): string {
  // If the lockfile records where the tarball lives, prefer that — scope
  // routing (`@scope:registry`) only covers scoped packages, but named
  // registries (`gh:`, `jsr:` aliases, custom) ship un-scoped packages whose
  // origin we'd otherwise miss.
  if (tarballUrl) {
    try {
      const origin = new URL(tarballUrl).origin
      const matched = namedRegistryOrigins.get(origin)
      if (matched) return matched
    } catch {
      // Fall through to scope routing.
    }
  }
  return pickRegistryForPackage(opts.registries, name)
}
