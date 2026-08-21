import { ABBREVIATED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import { clearMeta } from './clearMeta.js'
import {
  fetchMetadataFromFromRegistry,
  type FetchMetadataFromFromRegistryOptions,
  type FetchMetadataResult,
} from './fetch.js'
import { encodeMirror, fullEtagOfAbbreviatedMirror, holdsFullMetaInAbbreviatedMirror, mirrorEtags } from './metaMirror.js'
import {
  discardMirrorAfterFailedUncacheableWrite,
  getPkgMirrorPath,
  legacyMirrorHint,
  loadMeta,
  loadMetaHeaders,
  saveMeta,
} from './pickPackage.js'

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
  /**
   * Use only pnpm's on-disk metadata mirror and never reach the registry.
   */
  offline?: boolean
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
 * populates by default.
 */
export async function fetchAbbreviatedMetadataCached (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: FetchMetadataCachedOptions
): Promise<PackageMeta> {
  return fetchMetadataCached(fetchOpts, pkgName, { ...opts, fullMetadata: false, metaDir: ABBREVIATED_META_DIR })
}

type MetadataCacheRequest = FetchMetadataCachedOptions & { fullMetadata: boolean, metaDir: string }

async function fetchMetadataCached (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: MetadataCacheRequest
): Promise<PackageMeta> {
  const pkgMirror = opts.cacheDir != null
    ? getPkgMirrorPath(opts.cacheDir, opts.metaDir, opts.registry, pkgName)
    : null

  if (opts.offline === true) return loadOfflineMeta(pkgMirror, pkgName, opts)

  const cacheHeaders = pkgMirror != null ? await loadMetaHeaders(pkgMirror) : null
  const fullEtag = fullEtagOfAbbreviatedMirror(cacheHeaders, opts.fullMetadata)
  const conditional = await fetchMetadataFromFromRegistry(fetchOpts, pkgName, {
    registry: opts.registry,
    authHeaderValue: opts.authHeaderValue,
    cacheBypass: cacheHeaders?.uncacheable === true,
    fullMetadata: opts.fullMetadata || fullEtag != null,
    etag: fullEtag ?? cacheHeaders?.etag,
    modified: cacheHeaders?.modified,
  })
  if (!conditional.notModified) return persistFetchedMeta(pkgMirror, conditional, opts.fullMetadata)

  // A 304 only resolves as `notModified` when a validator was sent, which
  // requires cache headers loaded from a mirror — so a null mirror here is an
  // unreachable invariant breach.
  if (pkgMirror == null) throw new Error(`Unexpected 304 for ${pkgName} without a metadata cache`)
  const cached = await loadMeta(pkgMirror, { hydrateEagerly: true })
  if (cached != null) return cached

  // Either the mirror vanished between the headers read and this read
  // (concurrent store cleanup, antivirus, ...) or a version fragment in it is
  // corrupt, so the 304 now validates nothing. Ask again
  // as a cold cache would, which the registry can only answer with a body or an
  // error — never another 304.
  return persistFetchedMeta(pkgMirror, await refetchBypassingCache(fetchOpts, pkgName, opts), opts.fullMetadata)
}

async function loadOfflineMeta (
  pkgMirror: string | null,
  pkgName: string,
  opts: MetadataCacheRequest
): Promise<PackageMeta> {
  if (pkgMirror != null) {
    const cached = await loadMeta(pkgMirror, { hydrateEagerly: true })
    if (cached != null) return cached
  }
  throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${pkgName} in package mirror ${pkgMirror ?? ''}`, {
    hint: opts.cacheDir != null ? await legacyMirrorHint(opts.cacheDir, opts.metaDir, opts.registry, pkgName) : undefined,
  })
}

async function refetchBypassingCache (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: MetadataCacheRequest
): Promise<FetchMetadataResult> {
  const refetched = await fetchMetadataFromFromRegistry(fetchOpts, pkgName, {
    registry: opts.registry,
    authHeaderValue: opts.authHeaderValue,
    cacheBypass: true,
    fullMetadata: opts.fullMetadata,
  })
  // Unreachable narrowing guard: the cache-bypassing request sends no validator,
  // so fetchMetadataFromFromRegistry rejects a repeated 304 before returning.
  if (refetched.notModified) throw new Error(`Unexpected 304 for ${pkgName} on a cache-bypassing refetch`)
  return refetched
}

// Persist a freshly downloaded body so the next install can do a headers-only
// conditional GET, then hand its meta back. Fire-and-forget — a cache-write
// failure isn't a reason to fail the caller; the next install just won't get
// the speedup.
function persistFetchedMeta (pkgMirror: string | null, fetched: FetchMetadataResult, mirrorFullMetadata: boolean): PackageMeta {
  if (pkgMirror != null) {
    const { etag, fullEtag } = mirrorEtags(fetched, mirrorFullMetadata)
    const content = encodeMirror({}, fetched, {
      meta: holdsFullMetaInAbbreviatedMirror(fetched, mirrorFullMetadata) ? clearMeta(fetched.meta) : fetched.meta,
      etag,
      body: { uncacheable: fetched.uncacheable, fullEtag },
    })
    saveMeta(pkgMirror, content).catch(() => {
      return discardMirrorAfterFailedUncacheableWrite(pkgMirror, fetched.uncacheable === true)
    })
  }
  return fetched.meta
}
