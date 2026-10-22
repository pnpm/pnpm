import { ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'

import { mirrorMayAnswer } from './metadataCachePolicy.js'
import {
  getFileMtime,
  getPkgMetaCacheKey,
  getPkgMirrorPath,
  hasVersionManifest,
  isMalformedMirrorFragmentError,
  legacyMirrorHint,
  loadMetaHeaders,
  metaHeadersOf,
  runLimited,
} from './metaMirror.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import { pickFromRegistry } from './pickFromRegistry.js'
import {
  type PickerOptions,
  pickMatchingVersionFast,
  pickMatchingVersionFinal,
  pickVersionFromStore,
} from './pickMatchingVersion.js'
import {
  cachedMetaMissesPreferredVersion,
  getDominantLockfileVersion,
  pickStableCachedRangeVersion,
} from './pickPackageFromMeta.js'
import {
  loadMetaCondensed,
  loadSessionDiskMeta,
  type MirrorSession,
  type PackageMetaCache,
  type PickPackageContext,
  type PickPackageOptions,
  type PickRequest,
  type PickResult,
} from './pickRequest.js'
import { maybeUpgradeAbbreviatedMetaForReleaseAge, upgradeMetaForCache } from './releaseAgeUpgrade.js'
import {
  decideCacheHitCheck,
  decideMirrorShortcuts,
  isFreshUnvalidatedMirror,
  isMirrorWithoutEtag,
  mirrorWrittenAfter,
  readsMirrorFirst,
} from './route.js'
import { toRaw } from './toRaw.js'

export {
  discardMirrorAfterFailedUncacheableWrite,
  encodePkgName,
  getPkgMetaCacheKey,
  getPkgMirrorPath,
  legacyMirrorHint,
  loadMeta,
  loadMetaHeaders,
  prepareJsonForDisk,
  saveMeta,
} from './metaMirror.js'
export { warnMissingTimeFieldOnce } from './pickMatchingVersion.js'
export type { PackageMetaCache, PickPackageOptions } from './pickRequest.js'
export { UNVALIDATED_MIRROR_MAX_AGE_MS } from './route.js'

/**
 * Packuments promoted into the in-memory cache straight from the on-disk
 * mirror, without registry validation. The mirror may predate versions the
 * registry has, so when a cache hit on such an entry can't satisfy the
 * requested spec (and the resolver isn't offline), `pickPackage` falls
 * through to the regular flow — a conditional registry request — instead of
 * failing the pick, exactly as it would have before the entry was promoted.
 * Network-fetched and 304-revalidated packuments are never in this set, so
 * hits on them keep returning directly even when the pick fails (the caller
 * then falls back to workspace packages or reports no matching version).
 */
const unverifiedDiskPackuments = new WeakSet<PackageMeta>()

/**
 * Promote a packument parsed from the on-disk mirror into the in-memory
 * cache, so repeat resolutions of the same package (common across a large
 * dependency graph) don't re-read and re-parse the mirror. The entry is
 * remembered as disk-sourced (see {@link unverifiedDiskPackuments}) because it
 * never went through registry validation.
 */
function cacheDiskLoadedMeta (metaCache: PackageMetaCache, cacheKey: string, meta: PackageMeta): void {
  unverifiedDiskPackuments.add(meta)
  metaCache.set(cacheKey, meta)
}

function toPickerOptions (
  ctx: { ignoreMissingTimeField?: boolean },
  opts: PickPackageOptions
): PickerOptions {
  return {
    preferredVersionSelectors: opts.preferredVersionSelectors,
    publishedBy: opts.publishedBy,
    fallbackPublishedBy: opts.fallbackPublishedBy,
    publishedByExclude: opts.publishedByExclude,
    pickLowestVersion: opts.pickLowestVersion,
    includeLatestTag: opts.includeLatestTag,
    ignoreMissingTimeField: ctx.ignoreMissingTimeField,
  }
}

/**
 * Picks from a packument that {@link pickPackage} already fetched, applying
 * the same version preferences and `minimumReleaseAge` handling, so a caller
 * that narrows the packument after the first pick gets the pick the resolver
 * would have made had those versions never been published.
 */
export function pickPackageFromFetchedMeta (
  ctx: { ignoreMissingTimeField?: boolean },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions,
  meta: PackageMeta
): PackageInRegistry | null {
  return pickMatchingVersionFinal(toPickerOptions(ctx, opts), spec, meta)
}

export async function pickPackage (
  ctx: PickPackageContext,
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): Promise<PickResult> {
  opts = opts || {}
  const request = createPickRequest(ctx, spec, opts)
  const memoryCachedPick = await pickFromMemoryCache(request)
  if (memoryCachedPick != null) return memoryCachedPick
  return runLimited(request.pkgMirror, async (limit) => pickFromMirrorOrRegistry({ request, limit }))
}

function createPickRequest (
  ctx: PickPackageContext,
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): PickRequest {
  const pickerOpts = toPickerOptions(ctx, opts)

  validatePackageName(spec.name)

  // Use full metadata for optional dependencies to get libc field.
  // See: https://github.com/pnpm/pnpm/issues/9950
  // The per-registry answer is authoritative when the caller can give one: it
  // already folds in the reasons that hold for every registry, so a registry
  // that carries `time` is free to stay on abbreviated metadata while the
  // others do not.
  const policyWantsFullMetadata = ctx.needsFullMetadataFor?.(opts.registry) ?? ctx.fullMetadata === true
  const fullMetadata = opts.optional === true || policyWantsFullMetadata
  const metaDir = fullMetadata
    ? (ctx.filterMetadata ? FULL_FILTERED_META_DIR : FULL_META_DIR)
    : ABBREVIATED_META_DIR
  return {
    ctx,
    spec,
    opts,
    pickerOpts,
    fullMetadata,
    metaDir,
    cacheKey: getPkgMetaCacheKey(opts.registry, spec.name, fullMetadata, ctx.filterMetadata === true),
    pkgMirror: getPkgMirrorPath(ctx.cacheDir, metaDir, opts.registry, spec.name),
  }
}

/**
 * Serves the pick from the in-memory cache, or returns `undefined` when the
 * cached entry is a disk-promoted packument that can't satisfy the spec: the
 * pick then revalidates against the registry (see unverifiedDiskPackuments).
 */
async function pickFromMemoryCache (request: PickRequest): Promise<PickResult | undefined> {
  const { ctx, spec, opts } = request
  // updateChecksums must reach the conditional registry request, so it
  // can't be served from the in-memory cache — which may hold a disk-promoted
  // entry rather than a fresh network fetch (see the updateChecksums doc).
  const cachedMeta = opts.updateChecksums ? undefined : ctx.metaCache.get(request.cacheKey)
  if (cachedMeta == null) return undefined
  // The in-memory cache may hold abbreviated metadata from an earlier call
  // that didn't need `time` (no publishedBy then). If this call has
  // publishedBy and the package was modified recently, upgrade to full
  // metadata so the maturity check runs properly.
  const { meta } = await upgradeCachedMetaForReleaseAge(request, cachedMeta)
  let pickedPackage: PackageInRegistry | null
  let offlinePickedPackage: PackageInRegistry | null | undefined
  try {
    pickedPackage = pickMatchingVersionFinal(request.pickerOpts, spec, meta)
    offlinePickedPackage = ctx.offline === true
      ? await pickVersionFromStore(ctx, { pickerOpts: request.pickerOpts, spec, meta, pickedPackage })
      : undefined
  } catch (err: unknown) {
    // A disk-promoted entry with a corrupt fragment. Offline, the mirror pick
    // fails with NO_OFFLINE_META. Online, the 304 handler refetches past it.
    if (isMalformedMirrorFragmentError(err)) return undefined
    throw err
  }
  if (!cacheHitMayServe(request, { meta, pickedPackage }) || !canServeCachedMeta(ctx, meta)) return undefined
  return {
    meta,
    pickedPackage: offlinePickedPackage ?? pickedPackage,
  }
}

function cacheHitMayServe (
  request: PickRequest,
  { meta, pickedPackage }: { meta: PackageMeta, pickedPackage: PackageInRegistry | null }
): boolean {
  const verified = !unverifiedDiskPackuments.has(meta)
  switch (decideCacheHitCheck(request, verified, pickedPackage != null)) {
    case 'serve': return true
    case 'revalidate': return false
    case 'stableRangePick': {
      const { spec, opts } = request
      return pickedPackage?.version === getDominantLockfileVersion(spec.fetchSpec, opts.preferredVersionSelectors)
    }
  }
}

async function upgradeCachedMetaForReleaseAge (
  request: PickRequest,
  cachedMeta: PackageMeta
): Promise<{ meta: PackageMeta, upgraded: boolean }> {
  const { ctx, opts, pkgMirror, cacheKey } = request
  const upgrade = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, request, cachedMeta)
  const meta = upgradeMetaForCache(ctx, upgrade, { pkgMirror, dryRun: opts.dryRun })
  const upgraded = upgrade.upgradedFrom != null
  if (upgraded) {
    ctx.metaCache.set(cacheKey, meta)
  }
  return { meta, upgraded }
}

async function pickFromMirrorOrRegistry (session: MirrorSession): Promise<PickResult> {
  const { request } = session
  if (readsMirrorFirst(request)) {
    const picked = await pickFromMirrorPreferringOffline(session)
    if (picked != null) return picked
  }
  const shortcuts = decideMirrorShortcuts(request)
  return (
    (shortcuts.exactVersion ? await pickExactVersionFromMirror(session) : undefined) ??
    (shortcuts.dominantLockfileVersion ? await pickStableRangeFromMirror(session) : undefined) ??
    (shortcuts.unvalidatedMirror ? await pickFromFreshUnvalidatedMirror(session) : undefined) ??
    await pickFromMirrorWrittenAfterCutoff(session, shortcuts.releaseAgeApplies) ??
    pickFromRegistry(session)
  )
}

async function pickFromMirrorPreferringOffline (session: MirrorSession): Promise<PickResult | undefined> {
  const { ctx } = session.request
  session.diskMeta = await session.limit(async () => loadMirrorForOfflinePick(session.request))
  if (ctx.offline) return pickOffline(session.request, session.diskMeta)
  if (session.diskMeta == null) return undefined
  return pickFromUpgradedMirror(session, session.diskMeta)
}

// Concurrent offline picks of one package all miss the pre-queue cache
// check and queue behind this limiter, so the check is repeated inside
// the queue and the promotion happens before the limiter releases —
// otherwise every queued pick re-reads and re-parses the mirror. Serving
// a queued pick from the cache is equivalent to it having arrived after
// the first caller cached it: offline entries are always disk-sourced
// and maybeUpgradeAbbreviatedMetaForReleaseAge short-circuits when
// offline, so an in-memory hit returns this same meta with no network
// access.
async function loadMirrorForOfflinePick (request: PickRequest): Promise<PackageMeta | null> {
  const { ctx, cacheKey } = request
  if (ctx.offline !== true) return loadMetaCondensed(request)
  const cached = ctx.metaCache.get(cacheKey)
  if (cached != null) return cached
  const meta = await loadMetaCondensed(request)
  if (meta != null) {
    cacheDiskLoadedMeta(ctx.metaCache, cacheKey, meta)
  }
  return meta
}

async function pickOffline (request: PickRequest, diskMeta: PackageMeta | null): Promise<PickResult> {
  const { ctx, spec, opts } = request
  if (diskMeta == null) {
    throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${request.pkgMirror}`, {
      hint: await legacyMirrorHint(ctx.cacheDir, request.metaDir, opts.registry, spec.name),
    })
  }
  try {
    const pickedPackage = pickMatchingVersionFinal(request.pickerOpts, spec, diskMeta)
    const storePicked = await pickVersionFromStore(ctx, { pickerOpts: request.pickerOpts, spec, meta: diskMeta, pickedPackage })
    return {
      meta: diskMeta,
      pickedPackage: storePicked ?? pickedPackage,
    }
  } catch (err: unknown) {
    // A corrupt fragment makes the mirror as unusable offline as a missing
    // one, and there is no network to repair it from.
    if (!isMalformedMirrorFragmentError(err)) throw err
    throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${request.pkgMirror}`)
  }
}

async function pickFromUpgradedMirror (session: MirrorSession, diskMeta: PackageMeta): Promise<PickResult | undefined> {
  const { request } = session
  // Disk-cached meta may be abbreviated; upgrade for the maturity check
  // before letting pickMatchingVersionFinal warn-and-skip on missing time.
  const { meta, upgraded } = await upgradeCachedMetaForReleaseAge(request, diskMeta)
  session.diskMeta = meta
  let pickedPackage: PackageInRegistry | null
  try {
    pickedPackage = pickMatchingVersionFinal(request.pickerOpts, request.spec, meta)
  } catch (err: unknown) {
    // The registry request that follows replaces a corrupt mirror.
    if (isMalformedMirrorFragmentError(err)) return undefined
    throw err
  }
  if (!pickedPackage || !canServeCachedMeta(request.ctx, meta)) return undefined
  // A cache hit re-runs maybeUpgradeAbbreviatedMetaForReleaseAge, so
  // serving this meta from memory can't bypass the release-age
  // upgrade. When the upgrade branch above already cached the
  // registry-validated upgraded meta, don't overwrite it with a
  // disk-sourced marking.
  if (!upgraded) {
    cacheDiskLoadedMeta(request.ctx.metaCache, request.cacheKey, meta)
  }
  return { meta, pickedPackage }
}

async function pickExactVersionFromMirror (session: MirrorSession): Promise<PickResult | undefined> {
  const { ctx, spec } = session.request
  const diskMeta = await loadSessionDiskMeta(session)
  // use the cached meta only if it has the required package version
  // otherwise it is likely out of date
  if (
    diskMeta == null ||
    !canServeCachedMeta(ctx, diskMeta) ||
    !hasVersionManifest(diskMeta.versions ?? {}, spec.fetchSpec)
  ) {
    return undefined
  }
  return pickFromMirrorAndPromote(session.request, diskMeta)
}

/**
 * Picks from a mirror-read packument and promotes it to the in-memory cache
 * when the pick succeeds.
 */
function pickFromMirrorAndPromote (request: PickRequest, diskMeta: PackageMeta): PickResult | undefined {
  try {
    const pickedPackage = pickMatchingVersionFast(request.pickerOpts, request.spec, diskMeta)
    if (!pickedPackage) return undefined
    cacheDiskLoadedMeta(request.ctx.metaCache, request.cacheKey, diskMeta)
    return { meta: diskMeta, pickedPackage }
  } catch {
    // Swallow fast-path errors (e.g. ERR_PNPM_MISSING_TIME from
    // abbreviated meta) and fall through to the network fetch, which
    // can upgrade to full metadata and run the maturity check on
    // real `time` data.
    return undefined
  }
}

async function pickStableRangeFromMirror (session: MirrorSession): Promise<PickResult | undefined> {
  const { spec, opts } = session.request
  const dominantLockfileVersion = getDominantLockfileVersion(spec.fetchSpec, opts.preferredVersionSelectors)
  if (dominantLockfileVersion == null) return undefined
  const diskMeta = await loadSessionDiskMeta(session)
  if (diskMeta == null) return undefined
  try {
    return pickStableVersionAndPromote(session.request, diskMeta)
  } catch {
    // Any malformed cached metadata falls through to normal online
    // resolution, matching the neighboring disk fast paths.
    return undefined
  }
}

function pickStableVersionAndPromote (request: PickRequest, diskMeta: PackageMeta): PickResult | undefined {
  const { ctx, spec, opts } = request
  const stableVersion = pickStableCachedRangeVersion({
    meta: diskMeta,
    preferredVersionSelectors: opts.preferredVersionSelectors,
    versionRange: spec.fetchSpec,
  })
  if (stableVersion == null || !canServeCachedMeta(ctx, diskMeta)) return undefined
  // Strict dominance makes the preferred tier a singleton, so the
  // highest-version picker used by the proof and the normal picker
  // agree even if pickLowestVersion reaches this code in the future.
  const pickedPackage = pickMatchingVersionFast(request.pickerOpts, spec, diskMeta)
  if (pickedPackage == null || pickedPackage.version !== stableVersion) return undefined
  cacheDiskLoadedMeta(ctx.metaCache, request.cacheKey, diskMeta)
  return { meta: diskMeta, pickedPackage }
}

async function pickFromFreshUnvalidatedMirror (session: MirrorSession): Promise<PickResult | undefined> {
  const { spec, opts, pkgMirror } = session.request
  session.mirrorHeaders = session.diskMeta != null
    ? metaHeadersOf(session.diskMeta)
    : await session.limit(async () => loadMetaHeaders(pkgMirror))
  if (!isMirrorWithoutEtag(session.mirrorHeaders)) return undefined
  const mtime = await session.limit(async () => getFileMtime(pkgMirror))
  if (mtime == null || !isFreshUnvalidatedMirror(mtime, Date.now())) return undefined
  const diskMeta = await loadSessionDiskMeta(session)
  if (
    diskMeta == null ||
    cachedMetaMissesPreferredVersion(spec.fetchSpec, opts.preferredVersionSelectors, diskMeta)
  ) {
    return undefined
  }
  return pickFromMirrorAndPromote(session.request, diskMeta)
}

async function pickFromMirrorWrittenAfterCutoff (
  session: MirrorSession,
  releaseAgeApplies: boolean | undefined
): Promise<PickResult | undefined> {
  const { ctx, spec, pkgMirror } = session.request
  const cutoff = mirrorWrittenAfter(session.request, releaseAgeApplies)
  if (cutoff == null) return undefined
  const mtime = await session.limit(async () => getFileMtime(pkgMirror))
  if (!isWrittenAtOrAfter(mtime, cutoff)) return undefined
  const diskMeta = await loadSessionDiskMeta(session)
  if (diskMeta == null) return undefined
  try {
    const pickedPackage = pickMatchingVersionFast(session.request.pickerOpts, spec, diskMeta)
    if (pickedPackage && canServeCachedMeta(ctx, diskMeta)) {
      return { meta: diskMeta, pickedPackage }
    }
  } catch {
    // Same as the other mirror fast paths — fall through to the network fetch.
  }
  return undefined
}

function isWrittenAtOrAfter (mtime: Date | null, cutoff: Date): boolean {
  return mtime != null && mtime >= cutoff
}

function canServeCachedMeta (
  ctx: { offline?: boolean, preferOffline?: boolean },
  meta: PackageMeta
): boolean {
  return mirrorMayAnswer(ctx, meta.uncacheable)
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
