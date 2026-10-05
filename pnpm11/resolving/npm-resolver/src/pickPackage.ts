import { ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'

import {
  getFileMtime,
  getPkgMetaCacheKey,
  getPkgMirrorPath,
  hasVersionManifest,
  isMalformedMirrorFragmentError,
  legacyMirrorHint,
  loadMetaHeaders,
  type MetaHeaders,
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

function canReuseStableCachedRange (
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): boolean {
  return (
    spec.type === 'range' &&
    !opts.includeLatestTag &&
    !opts.updateChecksums &&
    opts.publishedBy == null &&
    opts.trustPolicy !== 'no-downgrade'
  )
}

/**
 * Registries that omit `ETag` cannot answer `If-None-Match` with 304, so a
 * warm revalidation downloads the whole packument. A mirror younger than this
 * and stored without an `ETag` is reused for a range the cache can already
 * satisfy. The public npm registry sends `ETag`s, so it keeps conditional
 * revalidation. After this age the mirror is fetched again, which is how a
 * version published in the meantime shows up.
 */
export const UNVALIDATED_MIRROR_MAX_AGE_MS = 5 * 60 * 1000

/**
 * The age is compared in both directions. A mirror dated far in the future,
 * for example after the clock was set back, has an unknown age and is not
 * reused. A few milliseconds of skew between the file system and the clock
 * are tolerated.
 */
function isYoungerThanUnvalidatedMirrorMaxAge (mtime: Date): boolean {
  return Math.abs(Date.now() - mtime.getTime()) < UNVALIDATED_MIRROR_MAX_AGE_MS
}

function canReuseFreshUnvalidatedMirror (
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): boolean {
  return canReuseStableCachedRange(spec, opts) && opts.refreshMetadata !== true
}

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
  const unverified = unverifiedDiskPackuments.has(meta)
  const unverifiedPickIsSafe = isUnverifiedPickSafe(request, { pickedPackage, unverified })
  const cacheResultCanReturn =
    ctx.offline === true ||
    !unverified ||
    (pickedPackage != null && unverifiedPickIsSafe)
  if (!cacheResultCanReturn || !canServeCachedMeta(ctx, meta)) return undefined
  return {
    meta,
    pickedPackage: offlinePickedPackage ?? pickedPackage,
  }
}

function isUnverifiedPickSafe (
  { ctx, spec, opts }: PickRequest,
  { pickedPackage, unverified }: { pickedPackage: PackageInRegistry | null, unverified: boolean }
): boolean {
  const stableCachedRangeVersion =
    unverified &&
    canReuseStableCachedRange(spec, opts)
      ? getDominantLockfileVersion(spec.fetchSpec, opts.preferredVersionSelectors)
      : null
  return (
    ctx.preferOffline === true ||
    opts.pickLowestVersion === true ||
    spec.type === 'version' ||
    (pickedPackage != null && pickedPackage.version === stableCachedRangeVersion)
  )
}

async function upgradeCachedMetaForReleaseAge (
  { ctx, spec, opts, pkgMirror, cacheKey }: PickRequest,
  cachedMeta: PackageMeta
): Promise<{ meta: PackageMeta, upgraded: boolean }> {
  const upgrade = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, cachedMeta)
  const meta = upgradeMetaForCache(ctx, upgrade, { pkgMirror, dryRun: opts.dryRun })
  const upgraded = upgrade.upgradedFrom != null
  if (upgraded) {
    ctx.metaCache.set(cacheKey, meta)
  }
  return { meta, upgraded }
}

async function pickFromMirrorOrRegistry (session: MirrorSession): Promise<PickResult> {
  return (
    await pickFromMirrorPreferringOffline(session) ??
    await pickExactVersionFromMirror(session) ??
    await pickStableRangeFromMirror(session) ??
    await pickFromFreshUnvalidatedMirror(session) ??
    await pickFromMirrorWrittenAfterCutoff(session) ??
    pickFromRegistry(session)
  )
}

async function pickFromMirrorPreferringOffline (session: MirrorSession): Promise<PickResult | undefined> {
  const { ctx, opts } = session.request
  if (ctx.offline !== true && ctx.preferOffline !== true && !opts.pickLowestVersion) return undefined
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
  const { ctx, spec, opts } = session.request
  if (opts.includeLatestTag || opts.updateChecksums || spec.type !== 'version') return undefined
  const diskMeta = await loadSessionDiskMeta(session)
  // use the cached meta only if it has the required package version
  // otherwise it is probably out of date
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
  const dominantLockfileVersion = canReuseStableCachedRange(spec, opts)
    ? getDominantLockfileVersion(spec.fetchSpec, opts.preferredVersionSelectors)
    : null
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
  if (!canReuseFreshUnvalidatedMirror(spec, opts)) return undefined
  session.mirrorHeaders = session.diskMeta != null
    ? metaHeadersOf(session.diskMeta)
    : await session.limit(async () => loadMetaHeaders(pkgMirror))
  if (!isReusableMirrorWithoutEtag(session.mirrorHeaders)) return undefined
  const mtime = await session.limit(async () => getFileMtime(pkgMirror))
  if (mtime == null || !isYoungerThanUnvalidatedMirrorMaxAge(mtime)) return undefined
  const diskMeta = await loadSessionDiskMeta(session)
  if (
    diskMeta == null ||
    cachedMetaMissesPreferredVersion(spec.fetchSpec, opts.preferredVersionSelectors, diskMeta)
  ) {
    return undefined
  }
  return pickFromMirrorAndPromote(session.request, diskMeta)
}

function isReusableMirrorWithoutEtag (headers: MetaHeaders | null): boolean {
  return (
    headers != null &&
    (headers.etag == null || headers.etag === '') &&
    headers.uncacheable !== true
  )
}

async function pickFromMirrorWrittenAfterCutoff (session: MirrorSession): Promise<PickResult | undefined> {
  const { ctx, spec, opts, pkgMirror } = session.request
  if (!opts.publishedBy || opts.publishedByExclude?.(spec.name) === true) return undefined
  const mtime = await session.limit(async () => getFileMtime(pkgMirror))
  if (!isWrittenAtOrAfter(mtime, opts.publishedBy)) return undefined
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

/**
 * Offline and prefer-offline may serve a mirror the registry marked
 * uncacheable. Every other online path has to refetch it. The flag is only
 * set on packuments read from the mirror: a document fetched during this
 * install stays reusable for the rest of it.
 */
function canServeCachedMeta (
  ctx: { offline?: boolean, preferOffline?: boolean },
  meta: PackageMeta
): boolean {
  return ctx.offline === true || ctx.preferOffline === true || meta.uncacheable !== true
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
