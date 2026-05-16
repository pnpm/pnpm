import path from 'node:path'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type DispatcherOptions } from '@pnpm/network.fetch'
import {
  encodePkgName,
  fetchMetadataFromFromRegistry,
  loadMeta,
  loadMetaHeaders,
  type PackageMeta,
  prepareJsonForDisk,
  saveMeta,
} from '@pnpm/resolving.npm-resolver'
import type { Registries, RegistryConfig } from '@pnpm/types'
import getRegistryName from 'encode-registry'

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
   * When set, the lookup issues conditional GETs against it: a 304 Not
   * Modified response serves the body from disk instead of refetching the
   * full document. On a 200, the new body is written back. Omitting it
   * disables caching — every revalidation downloads the full manifest.
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
    .map(normalizeRegistryUrl)
    .filter((value): value is string => value != null)
    .sort((a, b) => b.length - a.length)
  // In-memory dedup: the `time` map per (registry, name) for this install
  // only. Disk caching (and the conditional-GET fast path) is handled inside
  // fetchFullMetaTime via the resolver's shared metadata mirror at cacheDir.
  const cacheDir = opts.cacheDir
  const inflight = new Map<string, Promise<Record<string, string | undefined> | undefined>>()
  const fetchTimeMap = async (registry: string, name: string): Promise<Record<string, string | undefined> | undefined> => {
    const cacheKey = `${registry}\x00${name}`
    const cached = inflight.get(cacheKey)
    if (cached) return cached
    const promise = fetchFullMetaTime({
      cacheDir,
      fetchOpts,
      authHeaderValue: getAuthHeaderValueByURI(registry),
      name,
      registry,
    })
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
    const normalized = normalizeTarballUrl(tarballUrl)
    if (normalized) {
      for (const prefix of namedRegistryPrefixes) {
        if (normalized.startsWith(prefix)) return prefix
      }
    }
  }
  return pickRegistryForPackage(opts.registries, name)
}

interface FetchFullMetaTimeArgs {
  cacheDir: string | undefined
  fetchOpts: Parameters<typeof fetchMetadataFromFromRegistry>[0]
  authHeaderValue: string | undefined
  name: string
  registry: string
}

async function fetchFullMetaTime (args: FetchFullMetaTimeArgs): Promise<Record<string, string | undefined> | undefined> {
  const { cacheDir, fetchOpts, authHeaderValue, name, registry } = args
  // Same on-disk path the resolver uses for its full-metadata cache, so the
  // gate and the resolver share a single cache directory. Without this the
  // gate would re-download every (registry, name) on every install — the
  // performance hit raised in issue #11675.
  const pkgMirror = cacheDir != null
    ? path.join(cacheDir, FULL_META_DIR, getRegistryName(registry), `${encodePkgName(name)}.jsonl`)
    : null
  const cacheHeaders = pkgMirror != null ? await loadMetaHeaders(pkgMirror) : null
  const result = await fetchMetadataFromFromRegistry(fetchOpts, name, {
    registry,
    authHeaderValue,
    fullMetadata: true,
    etag: cacheHeaders?.etag,
    modified: cacheHeaders?.modified,
  })
  let meta: PackageMeta | null = null
  if ('notModified' in result && result.notModified) {
    if (pkgMirror == null) {
      // We didn't send conditional headers (no cacheDir), but the registry
      // returned 304 anyway. There's no body to fall back on.
      throw new PnpmError(
        'REVALIDATE_NOT_MODIFIED_WITHOUT_CACHE',
        `Registry returned 304 for ${name} without an existing cache to refresh.`
      )
    }
    meta = await loadMeta(pkgMirror)
    if (meta == null) {
      // Cache file vanished between header-load and meta-load (concurrent
      // store cleanup, antivirus, etc.). Treat as a soft miss rather than a
      // hard error — caller will surface this as manifest-unavailable.
      throw new PnpmError(
        'REVALIDATE_CACHE_MISSING_AFTER_304',
        `Metadata cache for ${name} disappeared between headers read and full read.`
      )
    }
  } else if ('meta' in result) {
    meta = result.meta
    if (pkgMirror != null) {
      // Persist so the next install can do a headers-only conditional GET.
      // Fire-and-forget — a cache-write failure isn't a reason to fail the
      // gate; the next install just won't get the speedup.
      const json = prepareJsonForDisk(meta, result.etag, result.jsonText)
      saveMeta(pkgMirror, json).catch(() => {})
    }
  }
  return meta?.time
}

function normalizeRegistryUrl (url: string): string | null {
  try {
    const parsed = new URL(url)
    // Ensure trailing slash so prefix matching against tarball URLs (which
    // always include the package path under the registry root) does not
    // accidentally match a sibling registry whose URL shares a prefix string.
    const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
    return `${parsed.origin}${pathname}`
  } catch {
    return null
  }
}

function normalizeTarballUrl (url: string): string | null {
  try {
    return new URL(url).toString()
  } catch {
    return null
  }
}
