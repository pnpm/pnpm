import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type DispatcherOptions } from '@pnpm/network.fetch'
import { fetchFullMetadataCached, fetchMetadataFromFromRegistry } from '@pnpm/resolving.npm-resolver'
import type { Registries, RegistryConfig } from '@pnpm/types'

import type { ManifestLookup, ManifestLookupResult } from './revalidateLockfileMinimumReleaseAge.js'

export interface CreateFullMetadataLookupOptions extends DispatcherOptions {
  registries: Registries
  /**
   * Registries reached via the named-registry resolver chain (e.g. `gh:` →
   * GitHub Packages). When a lockfile entry's tarball URL falls under one of
   * these registry base URLs, route the manifest fetch there instead of the
   * scope-derived default.
   */
  namedRegistries?: Record<string, string>
  configByUri?: Record<string, RegistryConfig>
  userAgent?: string
  retry?: Parameters<typeof fetchMetadataFromFromRegistry>[0]['retry']
  timeout?: number
  /**
   * Warn when a single manifest fetch takes longer than this many milliseconds.
   * Defaults to the same 10s threshold the regular resolver uses.
   */
  fetchWarnTimeoutMs?: number
  /**
   * pnpm's on-disk metadata cache (same directory the resolver writes to).
   * Forwarded to `fetchFullMetadataCached` so the gate issues conditional
   * GETs and reuses the resolver's mirror; omitting it disables caching and
   * every revalidation downloads the full manifest.
   */
  cacheDir?: string
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
    fetchWarnTimeoutMs: opts.fetchWarnTimeoutMs ?? 10_000,
  }
  // Pre-normalize named-registry URLs and sort by length so that when several
  // registries share a hostname (e.g. `https://npm.example.com/team-a/` vs
  // `https://npm.example.com/team-b/`) the lookup picks the longest matching
  // prefix — matching only `origin` would silently route to the wrong one.
  const namedRegistryPrefixes = Object.values(opts.namedRegistries ?? {})
    .map((url) => {
      const parsed = tryParseUrl(url)
      if (!parsed) return null
      // Ensure trailing slash so prefix matching against tarball URLs (which
      // always include the package path under the registry root) does not
      // accidentally match a sibling registry whose URL shares a prefix string.
      const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
      return `${parsed.origin}${pathname}`
    })
    .filter((value): value is string => value != null)
    .sort((a, b) => b.length - a.length)
  // In-memory dedup of the `time` map per (registry, name) for this install
  // only. The on-disk conditional-GET cache is handled inside
  // fetchFullMetadataCached via the resolver's shared mirror at opts.cacheDir.
  const cacheDir = opts.cacheDir
  const inflight = new Map<string, Promise<Record<string, string | undefined> | undefined>>()
  const fetchTimeMap = async (registry: string, name: string): Promise<Record<string, string | undefined> | undefined> => {
    const cacheKey = `${registry}\x00${name}`
    const cached = inflight.get(cacheKey)
    if (cached) return cached
    const promise = fetchFullMetadataCached(fetchOpts, name, {
      registry,
      authHeaderValue: getAuthHeaderValueByURI(registry),
      cacheDir,
    }).then((meta) => meta.time)
    inflight.set(cacheKey, promise)
    return promise
  }

  return async (name: string, version: string, tarballUrl?: string): Promise<ManifestLookupResult> => {
    const registry = pickRegistryForVersion(opts, namedRegistryPrefixes, name, tarballUrl)
    let time: Record<string, string | undefined> | undefined
    try {
      time = await fetchTimeMap(registry, name)
    } catch (err) {
      return {
        status: 'manifest-unavailable',
        reason: err instanceof Error ? err.message : String(err),
      }
    }
    const published = time?.[version]
    if (!published) {
      // For full metadata this means the version was removed from the manifest
      // (typically a deliberate unpublish) or the registry doesn't expose
      // per-version timestamps for it. Either way the release-age cannot be
      // verified, so report it as a violation rather than silently passing.
      return { status: 'version-not-in-manifest' }
    }
    return { status: 'ok', publishedAt: new Date(published) }
  }
}

function pickRegistryForVersion (
  opts: CreateFullMetadataLookupOptions,
  namedRegistryPrefixes: string[],
  name: string,
  tarballUrl: string | undefined
): string {
  // If the lockfile records where the tarball lives, prefer that — scope
  // routing (`@scope:registry`) only covers scoped packages, but named
  // registries (`gh:`, `jsr:` aliases, custom) ship un-scoped packages whose
  // origin we'd otherwise miss. Match the longest matching prefix so that two
  // named registries sharing a host but differing by path don't collide.
  if (tarballUrl) {
    const normalized = tryParseUrl(tarballUrl)?.toString()
    if (normalized) {
      for (const prefix of namedRegistryPrefixes) {
        if (normalized.startsWith(prefix)) return prefix
      }
    }
  }
  return pickRegistryForPackage(opts.registries, name)
}

function tryParseUrl (url: string): URL | null {
  try {
    return new URL(url)
  } catch {
    return null
  }
}
