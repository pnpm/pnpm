import { promises as fs } from 'node:fs'

import { logger } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'

import { type FetchMetadataResult, notModifiedWithoutCacheError } from './fetch.js'
import {
  condenseMetaForCache,
  encodeMirror,
  fullEtagOfAbbreviatedMirror,
  holdsFullMetaInAbbreviatedMirror,
  isMalformedMirrorFragmentError,
  loadMetaHeaders,
  type MetaHeaders,
  metaHeadersOf,
  mirrorEtags,
  saveMetaBestEffort,
} from './metaMirror.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import { pickMatchingVersionFinal } from './pickMatchingVersion.js'
import {
  loadMetaCondensed,
  loadSessionDiskMeta,
  type MirrorSession,
  type PickRequest,
  type PickResult,
} from './pickRequest.js'
import {
  maybeUpgradeAbbreviatedMetaForReleaseAge,
  parseModifiedDate,
  upgradeMetaForCache,
} from './releaseAgeUpgrade.js'

/**
 * Revalidates the mirror with a conditional registry request and picks from
 * the current packument. When the request fails, the pick falls back to
 * whatever the mirror holds.
 */
export async function pickFromRegistry (session: MirrorSession): Promise<PickResult> {
  const { request } = session
  try {
    const cacheHeaders = await readConditionalRequestHeaders(session)
    const conditional = await fetchConditionally(request, cacheHeaders)
    // `return await` (not `return`) so a failure inside persistFreshMeta lands
    // in this try's cached-meta fallback instead of escaping it.
    if (!conditional.notModified) return await persistFreshMeta(request, conditional)

    // 304: the cached mirror is still current.
    const diskMeta = await loadSessionDiskMeta(session)
    const validated = diskMeta != null ? await serveValidatedMirror(request, diskMeta) : undefined
    return validated ?? await refetchWithoutValidators(request)
  } catch (err: unknown) {
    return pickFromMirrorAfterFailedFetch(request, err)
  }
}

// Load only the cache headers (etag, modified) for conditional request headers.
// This avoids reading and parsing the full metadata file (which can be megabytes)
// when the registry returns 200 and the old metadata would be discarded anyway.
async function readConditionalRequestHeaders (session: MirrorSession): Promise<MetaHeaders | null> {
  if (session.diskMeta != null) return metaHeadersOf(session.diskMeta)
  if (session.mirrorHeaders !== undefined) return session.mirrorHeaders
  return session.limit(async () => loadMetaHeaders(session.request.pkgMirror))
}

async function fetchConditionally (
  { ctx, spec, opts, fullMetadata }: PickRequest,
  cacheHeaders: MetaHeaders | null
): ReturnType<PickRequest['ctx']['fetch']> {
  const fullEtag = fullEtagOfAbbreviatedMirror(cacheHeaders, fullMetadata)
  return ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    cacheBypass: cacheHeaders?.uncacheable === true,
    fullMetadata: fullMetadata || fullEtag != null,
    etag: fullEtag ?? cacheHeaders?.etag,
    modified: cacheHeaders?.modified,
    registry: opts.registry,
  })
}

// Either the mirror vanished between the headers read and this read
// (concurrent store cleanup, antivirus, ...) or its content turned out
// corrupt, so the 304 validates nothing. Ask
// again as a cold cache would, which the registry can only answer with a
// body or an error — never another 304.
async function refetchWithoutValidators (request: PickRequest): Promise<PickResult> {
  const { ctx, spec, opts } = request
  const refetched = await ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    cacheBypass: true,
    fullMetadata: request.fullMetadata,
    registry: opts.registry,
  })
  if (refetched.notModified) throw notModifiedWithoutCacheError(spec.name)
  return persistFreshMeta(request, refetched)
}

async function pickFromMirrorAfterFailedFetch (request: PickRequest, err: unknown): Promise<PickResult> {
  (err as { spec?: RegistryPackageSpec }).spec = request.spec
  const meta = await loadMetaCondensed(request)
  if (meta == null) throw err
  let pickedPackage: PackageInRegistry | null
  try {
    pickedPackage = pickMatchingVersionFinal(request.pickerOpts, request.spec, meta)
  } catch (pickErr: unknown) {
    // A corrupt fragment makes this fallback as useless as a missing mirror.
    if (isMalformedMirrorFragmentError(pickErr)) throw err
    throw pickErr
  }
  logger.debug({ message: `Using cached meta from ${request.pkgMirror}` })
  return { meta, pickedPackage }
}

/**
 * {@link serveValidatedMeta}, or `undefined` when a version fragment of the
 * local file is corrupt. The 304 validated the ETag in the intact headers
 * record, so without a refetch the mirror would keep revalidating and never
 * be repaired.
 */
async function serveValidatedMirror (request: PickRequest, cached: PackageMeta): Promise<PickResult | undefined> {
  try {
    return await serveValidatedMeta(request, cached)
  } catch (err: unknown) {
    if (isMalformedMirrorFragmentError(err)) return undefined
    throw err
  }
}

// A 304 whose cached body is still on disk: the registry vouched the
// packument is current, so restart its validation clock, upgrade
// abbreviated -> full when the maturity check needs `time`, and serve it.
async function serveValidatedMeta (request: PickRequest, cached: PackageMeta): Promise<PickResult> {
  const { ctx, spec, opts, pkgMirror } = request
  // The registry just vouched that the cached packument equals its current
  // one, so the validation clock restarts now: bump the mirror's mtime so
  // the publishedBy freshness shortcut can fire again on the next
  // install. Without this, a mirror older than minimumReleaseAge
  // re-validates on every subsequent install — a 304 never rewrites the
  // file. Fire-and-forget: a read-only cache dir only costs another
  // conditional request.
  if (!opts.dryRun) {
    const now = new Date()
    fs.utimes(pkgMirror, now, now).catch(() => {})
  }
  const upgrade = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, cached)
  const meta = upgradeMetaForCache(ctx, upgrade, { pkgMirror, dryRun: opts.dryRun })
  // Pick before caching, so a corrupt fragment never leaves the document in
  // the in-memory cache.
  const pickedPackage = pickMatchingVersionFinal(request.pickerOpts, spec, meta)
  ctx.metaCache.set(request.cacheKey, meta)
  return { meta, pickedPackage }
}

interface FreshMetaUpgrade {
  meta: PackageMeta
  resultToSave: FetchMetadataResult
  attemptedReleaseAgeUpgrade: boolean
}

// A freshly downloaded 200 body: when minimumReleaseAge needs the
// per-version `time` an abbreviated document omits, upgrade to full
// metadata; then filter, persist to the mirror, and cache it.
async function persistFreshMeta (request: PickRequest, fetched: FetchMetadataResult): Promise<PickResult> {
  const { ctx, opts } = request
  const upgrade = await upgradeFreshMetaForReleaseAge(request, fetched)
  const { resultToSave } = upgrade
  const meta = condenseMetaForCache(ctx, upgrade.meta)
  if (upgrade.attemptedReleaseAgeUpgrade) {
    ctx.releaseAgeUpgradeCheckedPackuments?.add(meta)
  }
  if (!opts.dryRun) {
    mirrorFreshMeta(request, { resultToSave, meta })
  }
  meta.etag = resultToSave.etag
  // only save meta to cache, when it is fresh
  ctx.metaCache.set(request.cacheKey, meta)
  return {
    meta,
    pickedPackage: pickMatchingVersionFinal(request.pickerOpts, request.spec, meta),
  }
}

// This two-step approach is intentional: abbreviated metadata is much smaller,
// and most packages won't have been modified recently enough to need the full
// document. We only upgrade to full metadata when the package's modification
// date is recent enough that some versions might not yet be "mature."
async function upgradeFreshMetaForReleaseAge (request: PickRequest, fetched: FetchMetadataResult): Promise<FreshMetaUpgrade> {
  const { ctx, spec, opts } = request
  if (!freshMetaNeedsFullForReleaseAge(request, fetched.meta)) {
    return { meta: fetched.meta, resultToSave: fetched, attemptedReleaseAgeUpgrade: false }
  }
  // Save the abbreviated metadata to the abbreviated cache before re-fetching full.
  if (!opts.dryRun) {
    const { etag, fullEtag } = mirrorEtags(fetched, request.fullMetadata)
    const content = encodeMirror(ctx, fetched, { meta: fetched.meta, etag, body: { uncacheable: fetched.uncacheable, fullEtag } })
    saveMetaBestEffort(request.pkgMirror, content, fetched.uncacheable === true)
  }
  const fullFetchResult = await ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    fullMetadata: true,
    registry: opts.registry,
  })
  if (fullFetchResult.notModified) {
    return { meta: fetched.meta, resultToSave: fetched, attemptedReleaseAgeUpgrade: true }
  }
  return { meta: fullFetchResult.meta, resultToSave: fullFetchResult, attemptedReleaseAgeUpgrade: true }
}

function freshMetaNeedsFullForReleaseAge ({ spec, opts, fullMetadata }: PickRequest, meta: PackageMeta): boolean {
  if (
    !opts.publishedBy ||
    fullMetadata ||
    meta.time != null ||
    opts.publishedByExclude?.(spec.name) === true
  ) {
    return false
  }
  const modifiedDate = parseModifiedDate(meta)
  // Strict `>` (not `>=`) so the boundary case `modified == publishedBy`
  // takes the abbreviated fast path: `modified` is an upper bound on
  // every version's publish time, so when it equals the cutoff every
  // version passes the per-version `<=` filter in
  // `filterPkgMetadataByPublishDate` and a full re-fetch isn't needed.
  return modifiedDate == null || modifiedDate > opts.publishedBy
}

function mirrorFreshMeta (
  request: PickRequest,
  { resultToSave, meta }: { resultToSave: FetchMetadataResult, meta: PackageMeta }
): void {
  // Mirror the fetched document, unless the retained form is deliberately
  // narrower: `filterMetadata` always mirrors the stripped document, and a
  // full document in the abbreviated slot mirrors the condensed form — `time`
  // is all the next install needs from this slot.
  const writeCondensed = request.ctx.filterMetadata === true ||
    (holdsFullMetaInAbbreviatedMirror(resultToSave, request.fullMetadata) && meta !== resultToSave.meta)
  const { etag, fullEtag } = mirrorEtags(resultToSave, request.fullMetadata)
  const content = encodeMirror(request.ctx, resultToSave, {
    meta: writeCondensed ? meta : resultToSave.meta,
    etag,
    body: { uncacheable: resultToSave.uncacheable, fullEtag },
  })
  saveMetaBestEffort(request.pkgMirror, content, resultToSave.uncacheable === true)
}
