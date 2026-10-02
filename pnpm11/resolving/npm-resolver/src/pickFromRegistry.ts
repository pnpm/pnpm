import { promises as fs } from 'node:fs'

import { logger } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import { type FetchMetadataResult, notModifiedWithoutCacheError } from './fetch.js'
import {
  condenseMetaForCache,
  fullEtagOfAbbreviatedMirror,
  holdsFullMetaInAbbreviatedMirror,
  loadMetaHeaders,
  type MetaHeaders,
  metaHeadersOf,
  mirrorEtags,
  prepareJsonForDisk,
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
    if (diskMeta != null) return await serveValidatedMeta(request, diskMeta)

    return await refetchWithoutValidators(request)
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
  const uncacheable = cacheHeaders?.uncacheable === true
  const fullEtag = fullEtagOfAbbreviatedMirror(cacheHeaders, fullMetadata)
  return ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    cacheBypass: uncacheable,
    fullMetadata: fullMetadata || fullEtag != null,
    etag: uncacheable ? undefined : (fullEtag ?? cacheHeaders?.etag),
    modified: uncacheable ? undefined : cacheHeaders?.modified,
    registry: opts.registry,
  })
}

// The mirror vanished between the headers read and this read (concurrent
// store cleanup, antivirus, ...), so the 304 now validates nothing. Ask
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
  logger.debug({ message: `Using cached meta from ${request.pkgMirror}` })
  return {
    meta,
    pickedPackage: pickMatchingVersionFinal(request.pickerOpts, request.spec, meta),
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
  // The cached metadata may be abbreviated (no per-version `time`). When
  // minimumReleaseAge is active we need `time` for the maturity check, so
  // upgrade to full metadata via a follow-up fetch when warranted. Without
  // this, repeat installs of recently-modified packages would silently
  // bypass the maturity check via the warn-and-skip fallback.
  const upgrade = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, cached)
  const meta = upgradeMetaForCache(ctx, upgrade, { pkgMirror, dryRun: opts.dryRun })
  ctx.metaCache.set(request.cacheKey, meta)
  return {
    meta,
    pickedPackage: pickMatchingVersionFinal(request.pickerOpts, spec, meta),
  }
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
    saveMetaBestEffort(request.pkgMirror, prepareJsonForDisk(fetched.meta, etag, { ...fetched, fullEtag }), fetched.uncacheable === true)
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
  // Mirror the raw registry body, unless the retained form is
  // deliberately narrower: `filterMetadata` always mirrors the stripped
  // document, and a full document in the abbreviated slot mirrors the
  // condensed form — `time` is all the next install needs from this slot.
  const writeCondensed = request.ctx.filterMetadata === true ||
    (holdsFullMetaInAbbreviatedMirror(resultToSave, request.fullMetadata) && meta !== resultToSave.meta)
  const { etag, fullEtag } = mirrorEtags(resultToSave, request.fullMetadata)
  const jsonForDisk = writeCondensed
    ? prepareJsonForDisk(meta, etag, { uncacheable: resultToSave.uncacheable, fullEtag })
    : prepareJsonForDisk(resultToSave.meta, etag, { ...resultToSave, fullEtag })
  saveMetaBestEffort(request.pkgMirror, jsonForDisk, resultToSave.uncacheable === true)
}
