import { ABBREVIATED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import {
  fetchMetadataFromFromRegistry,
  type FetchMetadataFromFromRegistryOptions,
  type FetchMetadataResult,
} from './fetch.js'
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
  const conditional = await fetchMetadataFromFromRegistry(fetchOpts, pkgName, {
    registry: opts.registry,
    authHeaderValue: opts.authHeaderValue,
    fullMetadata: opts.fullMetadata,
    etag: cacheHeaders?.etag,
    modified: cacheHeaders?.modified,
  })
  if (!conditional.notModified) return persistAndReturn(conditional)

  // A 304 only resolves as `notModified` when a validator was sent, which
  // requires cache headers loaded from a mirror — so a null mirror here is an
  // unreachable invariant breach.
  if (pkgMirror == null) throw new Error(`Unexpected 304 for ${pkgName} without a metadata cache`)
  const cached = await loadMeta(pkgMirror)
  if (cached != null) return cached

  // The mirror vanished between the headers read and this read (concurrent
  // store cleanup, antivirus, ...), so the 304 now validates nothing. Ask again
  // as a cold cache would, which the registry can only answer with a body or an
  // error — never another 304.
  const refetched = await fetchMetadataFromFromRegistry(fetchOpts, pkgName, {
    registry: opts.registry,
    authHeaderValue: opts.authHeaderValue,
    cacheBypass: true,
    fullMetadata: opts.fullMetadata,
  })
  // Unreachable narrowing guard: the cache-bypassing request sends no validator,
  // so fetchMetadataFromFromRegistry rejects a repeated 304 before returning.
  if (refetched.notModified) throw new Error(`Unexpected 304 for ${pkgName} on a cache-bypassing refetch`)
  return persistAndReturn(refetched)

  // Persist a freshly downloaded body so the next install can do a headers-only
  // conditional GET, then hand its meta back. Fire-and-forget — a cache-write
  // failure isn't a reason to fail the caller; the next install just won't get
  // the speedup.
  function persistAndReturn (fetched: FetchMetadataResult): PackageMeta {
    if (pkgMirror != null) {
      saveMeta(pkgMirror, prepareJsonForDisk(fetched.meta, fetched.etag, fetched.jsonText)).catch(() => {})
    }
    return fetched.meta
  }
}
