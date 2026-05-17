import { ABBREVIATED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import { fetchMetadataFromFromRegistry, type FetchMetadataFromFromRegistryOptions } from './fetch.js'
import { getPkgMirrorPath, loadMeta, loadMetaHeaders, prepareJsonForDisk, saveMeta } from './pickPackage.js'

export interface FetchMetadataCachedOptions {
  registry: string
  authHeaderValue?: string
  /**
   * pnpm's on-disk cache directory. When set, the call issues a conditional
   * GET against the matching mirror the resolver populates: a 304 Not
   * Modified response serves the body from disk, a 200 writes the new body
   * back. Omit to disable caching — every call re-fetches.
   */
  cacheDir?: string
}

export type FetchFullMetadataCachedOptions = FetchMetadataCachedOptions

/**
 * Fetch a full registry metadata document for `pkgName`, reusing pnpm's
 * shared on-disk metadata mirror when `cacheDir` is supplied. Built for the
 * `minimumReleaseAge` lockfile revalidation gate, which needs the `time`
 * field that abbreviated metadata omits; the cache reuse keeps repeat
 * installs from re-downloading the same multi-megabyte document for every
 * locked package.
 */
export async function fetchFullMetadataCached (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: FetchFullMetadataCachedOptions
): Promise<PackageMeta> {
  return fetchMetadataCached(fetchOpts, pkgName, { ...opts, fullMetadata: true, metaDir: FULL_META_DIR })
}

/**
 * Sibling of {@link fetchFullMetadataCached} that hits the abbreviated
 * metadata endpoint (`Accept: application/vnd.npm.install-v1+json`) and
 * caches under `ABBREVIATED_META_DIR` — the same mirror the resolver
 * populates by default. Used by the lockfile verification gate as a
 * cheap upper-bound check: if the package's `modified` field is older
 * than the policy cutoff, every version in it predates the cutoff and
 * no per-version timestamp lookup is needed.
 */
export async function fetchAbbreviatedMetadataCached (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: FetchMetadataCachedOptions
): Promise<PackageMeta> {
  return fetchMetadataCached(fetchOpts, pkgName, { ...opts, fullMetadata: false, metaDir: ABBREVIATED_META_DIR })
}

async function fetchMetadataCached (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: FetchMetadataCachedOptions & { fullMetadata: boolean, metaDir: string }
): Promise<PackageMeta> {
  const pkgMirror = opts.cacheDir != null
    ? getPkgMirrorPath(opts.cacheDir, opts.metaDir, opts.registry, pkgName)
    : null
  const cacheHeaders = pkgMirror != null ? await loadMetaHeaders(pkgMirror) : null
  const result = await fetchMetadataFromFromRegistry(fetchOpts, pkgName, {
    registry: opts.registry,
    authHeaderValue: opts.authHeaderValue,
    fullMetadata: opts.fullMetadata,
    etag: cacheHeaders?.etag,
    modified: cacheHeaders?.modified,
  })
  if ('notModified' in result && result.notModified) {
    if (pkgMirror == null) {
      // We didn't send conditional headers (no cacheDir), but the registry
      // returned 304 anyway. There's no body to fall back on.
      throw new PnpmError(
        'META_NOT_MODIFIED_WITHOUT_CACHE',
        `Registry returned 304 for ${pkgName} without an existing cache to refresh.`
      )
    }
    const meta = await loadMeta(pkgMirror)
    if (meta == null) {
      // Cache file vanished between header-load and meta-load (concurrent
      // store cleanup, antivirus, etc.).
      throw new PnpmError(
        'META_CACHE_MISSING_AFTER_304',
        `Metadata cache for ${pkgName} disappeared between headers read and full read.`
      )
    }
    return meta
  }
  if (pkgMirror != null) {
    // Persist so the next install can do a headers-only conditional GET.
    // Fire-and-forget — a cache-write failure isn't a reason to fail the
    // caller; the next install just won't get the speedup.
    const json = prepareJsonForDisk(result.meta, result.etag, result.jsonText)
    saveMeta(pkgMirror, json).catch(() => {})
  }
  return result.meta
}
