import { promises as fs } from 'node:fs'
import path from 'node:path'

import { ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import { globalWarn, logger } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import getRegistryName from 'encode-registry'
import pLimit, { type LimitFunction } from 'p-limit'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwrite } from 'rename-overwrite'
import semver from 'semver'

import { clearMeta, retainsFullMeta } from './clearMeta.js'
import {
  type FetchMetadataNotModifiedResult,
  type FetchMetadataResult,
  notModifiedWithoutCacheError,
} from './fetch.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import {
  pickLowestVersionByVersionRange,
  pickPackageFromMeta,
  type PickPackageFromMetaOptions,
  type PickPackageFromMetaResult,
  pickVersionByVersionRange,
} from './pickPackageFromMeta.js'
import { toRaw } from './toRaw.js'

export interface PackageMetaCache {
  /**
   * Must return the same object reference that `set` stored for the key: the
   * resolver tracks whether a cached packument was validated against the
   * registry by object identity (see `unverifiedDiskPackuments`). In a cache
   * that clones or deserializes on read, that provenance is lost and recovery
   * degrades — a stale disk-promoted entry that can't satisfy a spec fails
   * the pick instead of falling through to the registry.
   */
  get: (key: string) => PackageMeta | undefined
  set: (key: string, meta: PackageMeta) => void
  has: (key: string) => boolean
}

interface RefCountedLimiter {
  count: number
  limit: LimitFunction
}

/**
 * prevents simultaneous operations on the meta.json
 * otherwise it would cause EPERM exceptions
 */
const metafileOperationLimits = {} as {
  [pkgMirror: string]: RefCountedLimiter | undefined
}

/**
 * To prevent metafileOperationLimits from holding onto objects in memory on
 * the order of the number of packages, refcount the limiters and drop them
 * once they are no longer needed. Callers of this function should ensure
 * that the limiter is no longer referenced once fn's Promise has resolved.
 */
async function runLimited<T> (pkgMirror: string, fn: (limit: LimitFunction) => Promise<T>): Promise<T> {
  let entry!: RefCountedLimiter
  try {
    entry = metafileOperationLimits[pkgMirror] ??= { count: 0, limit: pLimit(1) }
    entry.count++
    return await fn(entry.limit)
  } finally {
    entry.count--
    if (entry.count === 0) {
      metafileOperationLimits[pkgMirror] = undefined
    }
  }
}

export interface PickPackageOptions extends PickPackageFromMetaOptions {
  authHeaderValue?: string
  pickLowestVersion?: boolean
  registry: string
  dryRun: boolean
  includeLatestTag?: boolean
  optional?: boolean
  /**
   * When true, force a conditional registry request so a stale on-disk
   * packument can't satisfy the call: the on-disk exact-version fast
   * path is skipped, and the in-memory cache is bypassed too. The fast
   * path now promotes disk-loaded packuments into the in-memory cache,
   * so an entry there can no longer be assumed to come from this
   * install's own fresh network fetch — on a shared or long-lived
   * resolver it might be disk-sourced, which would short-circuit the
   * revalidation updateChecksums exists to force.
   */
  updateChecksums?: boolean
}

interface PickerOptions extends PickPackageFromMetaOptions {
  pickLowestVersion?: boolean
  includeLatestTag?: boolean
  ignoreMissingTimeField?: boolean
}

type PickResult = PickPackageFromMetaResult

// When includeLatestTag is set, the "latest" dist-tag is added as a candidate
// alongside the requested spec, and the higher-versioned pick wins.
function runPicker (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  pickOne: (targetSpec: RegistryPackageSpec) => PickResult
): PickResult {
  const currentPkg = pickOne(spec)
  if (!pickerOpts.includeLatestTag) return currentPkg
  const latestPkg = pickOne({ ...spec, type: 'tag', fetchSpec: 'latest' })
  return pickMax(latestPkg, currentPkg)
}

// Returns whichever pick has the higher version; treats a null `package` as
// "no match" but still propagates `latest` (the policy-aware tag) so the
// caller can surface it even when no candidate satisfied the spec.
function pickMax (a: PickResult, b: PickResult): PickResult {
  if (!a.package) return b
  if (!b.package) return a
  return semver.lt(a.package.version, b.package.version) ? b : a
}

const pickHighest = pickPackageFromMeta.bind(null, pickVersionByVersionRange)
const pickLowest = pickPackageFromMeta.bind(null, pickLowestVersionByVersionRange)

// When minimumReleaseAge is active: try the highest mature version; if none
// satisfies the range, fall back to the lowest version regardless of maturity
// so the resolver can report the violation inline and let the install layer
// (or other caller) decide what to do — never throw at this layer.
function pickRespectingMinReleaseAge (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PickResult {
  return runPicker(pickerOpts, spec, (targetSpec) => {
    const highest = pickHighest(pickerOpts, meta, targetSpec)
    if (highest.package) return highest
    return pickLowest({
      preferredVersionSelectors: pickerOpts.preferredVersionSelectors,
    }, meta, targetSpec)
  })
}

// When minimumReleaseAge is not active: pick by pickLowestVersion preference.
function pickIgnoringReleaseAge (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PickResult {
  const pickVersion = pickerOpts.pickLowestVersion ? pickLowest : pickHighest
  return runPicker(pickerOpts, spec, (targetSpec) => pickVersion(pickerOpts, meta, targetSpec))
}

// Used in shortcut/fall-through paths: if it fails (including with
// ERR_PNPM_MISSING_TIME), the caller falls through to the next path — e.g.
// the network fetch that can upgrade abbreviated metadata to full.
function pickMatchingVersionFast (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PickResult {
  return pickerOpts.publishedBy
    ? pickRespectingMinReleaseAge(pickerOpts, spec, meta)
    : pickIgnoringReleaseAge(pickerOpts, spec, meta)
}

// Used at terminal return sites where no further fallback path exists. When
// metadata lacks the per-version `time` field and ignoreMissingTimeField is
// enabled, skip the minimumReleaseAge filter with a warning instead of
// failing hard.
function pickMatchingVersionFinal (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PickResult {
  try {
    return pickMatchingVersionFast(pickerOpts, spec, meta)
  } catch (err: unknown) {
    if (pickerOpts.ignoreMissingTimeField && isMissingTimeError(err)) {
      warnMissingTimeFieldOnce(meta.name)
      return pickMatchingVersionFast({
        ...pickerOpts,
        publishedBy: undefined,
        publishedByExclude: undefined,
      }, spec, meta)
    }
    throw err
  }
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

/**
 * The form in which a packument is retained in memory (see {@link clearMeta}
 * for why). Full documents reach even a plain install via optional
 * dependencies (fetched full for `libc`), release-age `time` upgrades, and
 * mirror files that hold a full body.
 */
function condenseMetaForCache (
  ctx: { fullMetadata?: boolean, filterMetadata?: boolean },
  meta: PackageMeta
): PackageMeta {
  return retainsFullMeta(ctx) ? meta : clearMeta(meta)
}

export async function pickPackage (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, cacheBypass?: boolean, fullMetadata?: boolean, etag?: string, modified?: string }) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>
    fullMetadata?: boolean
    metaCache: PackageMetaCache
    cacheDir: string
    offline?: boolean
    preferOffline?: boolean
    filterMetadata?: boolean
    ignoreMissingTimeField?: boolean
  },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null, latest?: string }> {
  opts = opts || {}

  const pickerOpts: PickerOptions = {
    preferredVersionSelectors: opts.preferredVersionSelectors,
    publishedBy: opts.publishedBy,
    publishedByExclude: opts.publishedByExclude,
    pickLowestVersion: opts.pickLowestVersion,
    includeLatestTag: opts.includeLatestTag,
    ignoreMissingTimeField: ctx.ignoreMissingTimeField,
  }

  validatePackageName(spec.name)

  // Use full metadata for optional dependencies to get libc field.
  // See: https://github.com/pnpm/pnpm/issues/9950
  const fullMetadata = opts.optional === true || ctx.fullMetadata === true
  const metaDir = fullMetadata
    ? (ctx.filterMetadata ? FULL_FILTERED_META_DIR : FULL_META_DIR)
    : ABBREVIATED_META_DIR
  // Cache key includes the registry so a package of the same name served by two
  // registries in one install can't share a slot (which would resolve the wrong
  // tarball/integrity), plus fullMetadata/filterMetadata so a request is never
  // served a less-detailed or differently-stripped document than it asked for.
  const cacheKey = getPkgMetaCacheKey(opts.registry, spec.name, fullMetadata, ctx.filterMetadata === true)
  const pkgMirror = getPkgMirrorPath(ctx.cacheDir, metaDir, opts.registry, spec.name)
  // updateChecksums must reach the conditional registry request below, so it
  // can't be served from the in-memory cache — which may hold a disk-promoted
  // entry rather than a fresh network fetch (see the updateChecksums doc).
  const cachedMeta = opts.updateChecksums ? undefined : ctx.metaCache.get(cacheKey)
  if (cachedMeta != null) {
    // The in-memory cache may hold abbreviated metadata from an earlier call
    // that didn't need `time` (no publishedBy then). If this call has
    // publishedBy and the package was modified recently, upgrade to full
    // metadata so the maturity check runs properly.
    const upgrade = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, cachedMeta)
    const metaForCache = upgradeMetaForCache(ctx, upgrade, { pkgMirror, dryRun: opts.dryRun })
    if (upgrade.upgradedFrom != null) {
      ctx.metaCache.set(cacheKey, metaForCache)
    }
    const picked = pickMatchingVersionFinal(pickerOpts, spec, metaForCache)
    if (picked.package != null || ctx.offline === true || !unverifiedDiskPackuments.has(metaForCache)) {
      return {
        meta: metaForCache,
        pickedPackage: picked.package,
        latest: picked.latest,
      }
    }
    // Disk-promoted meta that can't satisfy the spec: fall through and
    // revalidate against the registry (see unverifiedDiskPackuments).
  }

  return runLimited(pkgMirror, async (limit) => {
    const loadMetaCondensed = async (): Promise<PackageMeta | null> => {
      const meta = await loadMeta(pkgMirror)
      return meta == null ? null : condenseMetaForCache(ctx, meta)
    }
    let diskMeta: PackageMeta | null | undefined
    if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
      diskMeta = await limit(loadMetaCondensed)

      if (ctx.offline) {
        if (diskMeta != null) {
          // maybeUpgradeAbbreviatedMetaForReleaseAge short-circuits when
          // offline, so a later in-memory cache hit returns this same meta
          // without any network access.
          cacheDiskLoadedMeta(ctx.metaCache, cacheKey, diskMeta)
          const picked = pickMatchingVersionFinal(pickerOpts, spec, diskMeta)
          return {
            meta: diskMeta,
            pickedPackage: picked.package,
            latest: picked.latest,
          }
        }

        throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
      }

      if (diskMeta != null) {
        // Disk-cached meta may be abbreviated; upgrade for the maturity check
        // before letting pickMatchingVersionFinal warn-and-skip on missing time.
        const upgrade = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, diskMeta)
        diskMeta = upgradeMetaForCache(ctx, upgrade, { pkgMirror, dryRun: opts.dryRun })
        if (upgrade.upgradedFrom != null) {
          ctx.metaCache.set(cacheKey, diskMeta)
        }
        const picked = pickMatchingVersionFinal(pickerOpts, spec, diskMeta)
        if (picked.package) {
          // A cache hit re-runs maybeUpgradeAbbreviatedMetaForReleaseAge, so
          // serving this meta from memory can't bypass the release-age
          // upgrade. When the upgrade branch above already cached the
          // registry-validated upgraded meta, don't overwrite it with a
          // disk-sourced marking.
          if (upgrade.upgradedFrom == null) {
            cacheDiskLoadedMeta(ctx.metaCache, cacheKey, diskMeta)
          }
          return {
            meta: diskMeta,
            pickedPackage: picked.package,
            latest: picked.latest,
          }
        }
      }
    }

    if (!opts.includeLatestTag && !opts.updateChecksums && spec.type === 'version') {
      diskMeta = diskMeta ?? await limit(loadMetaCondensed)
      // use the cached meta only if it has the required package version
      // otherwise it is probably out of date
      if ((diskMeta?.versions?.[spec.fetchSpec]) != null) {
        try {
          const picked = pickMatchingVersionFast(pickerOpts, spec, diskMeta)
          if (picked.package) {
            cacheDiskLoadedMeta(ctx.metaCache, cacheKey, diskMeta)
            return {
              meta: diskMeta,
              pickedPackage: picked.package,
              latest: picked.latest,
            }
          }
        } catch {
          // Swallow fast-path errors (e.g. ERR_PNPM_MISSING_TIME from
          // abbreviated meta) and fall through to the network fetch, which
          // can upgrade to full metadata and run the maturity check on
          // real `time` data.
        }
      }
    }
    if (opts.publishedBy && opts.publishedByExclude?.(spec.name) !== true) {
      const mtime = await limit(async () => getFileMtime(pkgMirror))
      if (mtime != null && mtime >= opts.publishedBy) {
        diskMeta = diskMeta ?? await limit(loadMetaCondensed)
        if (diskMeta != null) {
          try {
            const picked = pickMatchingVersionFast(pickerOpts, spec, diskMeta)
            if (picked.package) {
              return {
                meta: diskMeta,
                pickedPackage: picked.package,
                latest: picked.latest,
              }
            }
          } catch {
            // Same as above — fall through to the network fetch.
          }
        }
      }
    }

    try {
      // Load only the cache headers (etag, modified) for conditional request headers.
      // This avoids reading and parsing the full metadata file (which can be megabytes)
      // when the registry returns 200 and the old metadata would be discarded anyway.
      const cacheHeaders = diskMeta != null
        ? { etag: diskMeta.etag, modified: diskMeta.modified ?? diskMeta.time?.modified }
        : await limit(async () => loadMetaHeaders(pkgMirror))
      const conditional = await ctx.fetch(spec.name, {
        authHeaderValue: opts.authHeaderValue,
        fullMetadata,
        etag: cacheHeaders?.etag,
        modified: cacheHeaders?.modified,
        registry: opts.registry,
      })
      // `return await` (not `return`) so a failure inside persistFreshMeta lands
      // in this try's cached-meta fallback instead of escaping it.
      if (!conditional.notModified) return await persistFreshMeta(conditional)

      // 304: the cached mirror is still current.
      diskMeta = diskMeta ?? await limit(loadMetaCondensed)
      if (diskMeta != null) return await serveValidatedMeta(diskMeta)

      // The mirror vanished between the headers read and this read (concurrent
      // store cleanup, antivirus, ...), so the 304 now validates nothing. Ask
      // again as a cold cache would, which the registry can only answer with a
      // body or an error — never another 304.
      const refetched = await ctx.fetch(spec.name, {
        authHeaderValue: opts.authHeaderValue,
        cacheBypass: true,
        fullMetadata,
        registry: opts.registry,
      })
      if (refetched.notModified) throw notModifiedWithoutCacheError(spec.name)
      return await persistFreshMeta(refetched)
    } catch (err: any) { // eslint-disable-line
      err.spec = spec
      const meta = await loadMetaCondensed() // TODO: add test for this usecase
      if (meta == null) throw err
      logger.error(err, err)
      logger.debug({ message: `Using cached meta from ${pkgMirror}` })
      const picked = pickMatchingVersionFinal(pickerOpts, spec, meta)
      return {
        meta,
        pickedPackage: picked.package,
        latest: picked.latest,
      }
    }

    // A 304 whose cached body is still on disk: the registry vouched the
    // packument is current, so restart its validation clock, upgrade
    // abbreviated -> full when the maturity check needs `time`, and serve it.
    async function serveValidatedMeta (cached: PackageMeta): Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null, latest?: string }> {
      // The registry just vouched that the cached packument equals its current
      // one, so the validation clock restarts now: bump the mirror's mtime so
      // the publishedBy freshness shortcut above can fire again on the next
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
      ctx.metaCache.set(cacheKey, meta)
      const picked = pickMatchingVersionFinal(pickerOpts, spec, meta)
      return {
        meta,
        pickedPackage: picked.package,
        latest: picked.latest,
      }
    }

    // A freshly downloaded 200 body: when minimumReleaseAge needs the
    // per-version `time` an abbreviated document omits, upgrade to full
    // metadata; then filter, persist to the mirror, and cache it.
    async function persistFreshMeta (fetched: FetchMetadataResult): Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null, latest?: string }> {
      let meta = fetched.meta
      let resultToSave: FetchMetadataResult = fetched

      // This two-step approach is intentional: abbreviated metadata is much smaller,
      // and most packages won't have been modified recently enough to need the full
      // document. We only upgrade to full metadata when the package's modification
      // date is recent enough that some versions might not yet be "mature."
      if (
        opts.publishedBy &&
        !fullMetadata &&
        meta.time == null &&
        opts.publishedByExclude?.(spec.name) !== true
      ) {
        const modifiedDate = meta.modified ? new Date(meta.modified) : null
        const isModifiedValid = modifiedDate != null && !Number.isNaN(modifiedDate.getTime())
        // Strict `>` (not `>=`) so the boundary case `modified == publishedBy`
        // takes the abbreviated fast path: `modified` is an upper bound on
        // every version's publish time, so when it equals the cutoff every
        // version passes the per-version `<=` filter in
        // `filterPkgMetadataByPublishDate` and a full re-fetch isn't needed.
        if (!isModifiedValid || modifiedDate > opts.publishedBy) {
          // Save the abbreviated metadata to the abbreviated cache before re-fetching full.
          if (!opts.dryRun) {
            saveMetaBestEffort(pkgMirror, prepareJsonForDisk(resultToSave.meta, resultToSave.etag, resultToSave.jsonText))
          }
          const fullFetchResult = await ctx.fetch(spec.name, {
            authHeaderValue: opts.authHeaderValue,
            fullMetadata: true,
            registry: opts.registry,
          })
          if (!fullFetchResult.notModified) {
            resultToSave = fullFetchResult
            meta = fullFetchResult.meta
          }
        }
      }

      meta = condenseMetaForCache(ctx, meta)
      if (!opts.dryRun) {
        // Mirror the raw registry body, unless the retained form is
        // deliberately narrower: `filterMetadata` always mirrors the stripped
        // document, and an upgraded-to-full document mirrors the condensed
        // form — `time` is all the next install needs from this slot.
        const writeCondensed = ctx.filterMetadata === true || (resultToSave !== fetched && meta !== resultToSave.meta)
        const jsonForDisk = writeCondensed
          ? prepareJsonForDisk(meta, resultToSave.etag)
          : prepareJsonForDisk(resultToSave.meta, resultToSave.etag, resultToSave.jsonText)
        saveMetaBestEffort(pkgMirror, jsonForDisk)
      }
      meta.etag = resultToSave.etag
      // only save meta to cache, when it is fresh
      ctx.metaCache.set(cacheKey, meta)
      const picked = pickMatchingVersionFinal(pickerOpts, spec, meta)
      return {
        meta,
        pickedPackage: picked.package,
        latest: picked.latest,
      }
    }
  })
}

// When `minimumReleaseAge` is active and we have abbreviated metadata (which
// the npm registry serves by default and which omits per-version `time`),
// the maturity check can't run on the data we have. If the package has been
// modified since the maturity cutoff, re-fetch with `fullMetadata: true` so
// `time` is populated and the check can proceed properly. Without this,
// `pickMatchingVersionFinal` would fall back to its warn-and-skip path,
// silently bypassing the minimumReleaseAge guarantee for affected packages.
//
// Returns the original meta when no upgrade is needed. When an upgrade
// happens, returns both the upgraded meta and the underlying fetch result
// so callers can persist it to disk and avoid re-fetching on next install.
async function maybeUpgradeAbbreviatedMetaForReleaseAge (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, cacheBypass?: boolean, fullMetadata?: boolean, etag?: string, modified?: string }) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>
    offline?: boolean
  },
  spec: RegistryPackageSpec,
  opts: {
    publishedBy?: Date
    publishedByExclude?: PickPackageFromMetaOptions['publishedByExclude']
    authHeaderValue?: string
    registry: string
  },
  meta: PackageMeta
): Promise<{ meta: PackageMeta, upgradedFrom?: FetchMetadataResult }> {
  if (
    ctx.offline === true ||
    !opts.publishedBy ||
    meta.time != null ||
    opts.publishedByExclude?.(spec.name) === true
  ) {
    return { meta }
  }
  const modifiedDate = meta.modified ? new Date(meta.modified) : null
  const isModifiedValid = modifiedDate != null && !Number.isNaN(modifiedDate.getTime())
  if (isModifiedValid && modifiedDate <= opts.publishedBy) {
    // The package was last modified at or before the maturity cutoff. Since
    // `modified` is an upper bound on every version's publish time, no version
    // can be newer than the cutoff, so the abbreviated form is fine.
    // Inclusive at the boundary on purpose: matches the per-version `<=` filter
    // in `filterPkgMetadataByPublishDate`.
    return { meta }
  }
  // When `modified` is missing or malformed we fall through to the upgrade
  // fetch: prefer correctness (run the maturity check on real `time` data)
  // over saving a network call when our cached freshness signal is unusable.
  // Forward etag/modified so the registry can answer 304 if the upgraded
  // representation hasn't actually changed (rare on the npm registry where
  // full and abbreviated have distinct etags, but cheap to support).
  const fullFetchResult = await ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    fullMetadata: true,
    etag: meta.etag,
    modified: meta.modified,
    registry: opts.registry,
  })
  if (fullFetchResult.notModified) {
    // Upgrade fetch came back 304: keep the abbreviated meta. The downstream
    // `pickMatchingVersionFinal` will fall through to its warn-and-skip path.
    return { meta }
  }
  return { meta: fullFetchResult.meta, upgradedFrom: fullFetchResult }
}

/**
 * The meta to retain after a release-age upgrade check, persisted to the
 * mirror (unless dry-run) because the mirror otherwise still holds the
 * pre-upgrade abbreviated form without `time`, and every future install
 * would re-trigger the upgrade fetch.
 */
function upgradeMetaForCache (
  ctx: { fullMetadata?: boolean, filterMetadata?: boolean },
  upgrade: { meta: PackageMeta, upgradedFrom?: FetchMetadataResult },
  opts: { pkgMirror: string, dryRun: boolean }
): PackageMeta {
  if (upgrade.upgradedFrom == null) return upgrade.meta
  if (opts.dryRun) return condenseMetaForCache(ctx, upgrade.meta)
  return persistUpgradedMeta(ctx, opts.pkgMirror, upgrade.upgradedFrom)
}

// A condensing resolver keeps and mirrors the condensed form — the mirror
// only has to carry `time` into the next install; otherwise the raw response
// body is written and the unstripped meta is kept.
function persistUpgradedMeta (
  ctx: { fullMetadata?: boolean, filterMetadata?: boolean },
  pkgMirror: string,
  upgradedFrom: FetchMetadataResult
): PackageMeta {
  const metaForCache = condenseMetaForCache(ctx, upgradedFrom.meta)
  const jsonForDisk = metaForCache === upgradedFrom.meta
    ? prepareJsonForDisk(upgradedFrom.meta, upgradedFrom.etag, upgradedFrom.jsonText)
    : prepareJsonForDisk(metaForCache, upgradedFrom.etag)
  saveMetaBestEffort(pkgMirror, jsonForDisk)
  return metaForCache
}

/**
 * The mirror is an optimization, so a write failure only gets a debug log
 * with the mirror path and the install continues.
 */
function saveMetaBestEffort (pkgMirror: string, json: string): void {
  void runLimited(pkgMirror, (limit) => limit(async () => {
    try {
      await saveMeta(pkgMirror, json)
    } catch (err: unknown) {
      logger.debug({ message: `Failed to write the package metadata mirror at ${pkgMirror}`, err })
    }
  }))
}

export function encodePkgName (pkgName: string): string {
  if (pkgName !== pkgName.toLowerCase()) {
    return `${pkgName}_${createHexHash(pkgName)}`
  }
  return pkgName
}

/**
 * Key for the in-memory `metaCache` holding a package's registry metadata. The
 * registry is part of the key so that a package of the same name served by two
 * registries in one install can't collide on a single slot (which would resolve
 * the wrong tarball/integrity). `fullMetadata` and `filterMetadata` keep the
 * abbreviated, full, and filtered-full documents in distinct slots, mirroring
 * the on-disk `metaDir` split: a `filterMetadata` resolver stores a `clearMeta`-
 * stripped packument, so it must not share a slot with an unfiltered full one
 * (reachable only when a `metaCache` is shared across resolvers with different
 * settings). `filterMetadata` only narrows the full slot — abbreviated metadata
 * shares one on-disk mirror regardless, so its key carries no filtered variant.
 * `\x00` can't appear in a registry URL or a package name, so it's an
 * unambiguous separator. The verifier reads this same cache and must build the
 * key with this function.
 *
 * The registry is canonicalized to its origin plus a trailing-slashed path, so
 * the resolver (which may pass a configured named-registry URL verbatim) and
 * the verifier (which routes through trailing-slashed prefixes) converge on one
 * key for the same logical registry instead of creating duplicate slots. Origin
 * and path are preserved, so two registries that genuinely differ never collapse.
 */
export function getPkgMetaCacheKey (registry: string, pkgName: string, fullMetadata: boolean, filterMetadata: boolean): string {
  const key = `${canonicalizeRegistry(registry)}\x00${pkgName}`
  if (!fullMetadata) return key
  return filterMetadata ? `${key}:full:filtered` : `${key}:full`
}

function canonicalizeRegistry (registry: string): string {
  try {
    const parsed = new URL(registry)
    const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
    return `${parsed.origin}${pathname}`
  } catch {
    return registry
  }
}

/**
 * Path of the on-disk JSONL document where pnpm mirrors a package's registry
 * metadata. `metaDir` selects between abbreviated and full caches.
 */
export function getPkgMirrorPath (cacheDir: string, metaDir: string, registry: string, pkgName: string): string {
  return path.join(cacheDir, metaDir, getRegistryName(registry), `${encodePkgName(pkgName)}.jsonl`)
}

/**
 * Formats metadata for disk storage as two-line NDJSON:
 *   Line 1: cache headers (etag, modified) — small, fast to read
 *   Line 2: the registry metadata JSON
 *
 * The etag lives only in the headers line (`loadMeta` re-attaches it from
 * there), so a `meta` that carries one is serialized without it.
 */
export function prepareJsonForDisk (meta: PackageMeta, etag: string | undefined, jsonText?: string): string {
  const modified = meta.modified ?? meta.time?.modified
  const headers = JSON.stringify({ etag, modified })
  const body = jsonText ?? JSON.stringify(meta.etag == null ? meta : { ...meta, etag: undefined })
  return `${headers}\n${body}`
}

function isMissingTimeError (err: unknown): boolean {
  return (
    err != null &&
    typeof err === 'object' &&
    'code' in err &&
    (err as { code: string }).code === 'ERR_PNPM_MISSING_TIME'
  )
}

// Cap the size so long-lived processes (daemons, store servers) can't leak
// memory via this Set as they resolve ever more distinct packages.
const MAX_WARNED_MISSING_TIME = 1024
const warnedMissingTimeFor = new Set<string>()

export function warnMissingTimeFieldOnce (pkgName: string): void {
  if (warnedMissingTimeFor.has(pkgName)) return
  if (warnedMissingTimeFor.size >= MAX_WARNED_MISSING_TIME) {
    // Set preserves insertion order, so the first entry is the oldest.
    const oldest = warnedMissingTimeFor.values().next().value
    if (oldest != null) warnedMissingTimeFor.delete(oldest)
  }
  warnedMissingTimeFor.add(pkgName)
  globalWarn(`The metadata of ${pkgName} is missing the "time" field; skipping the minimumReleaseAge check for this package.`)
}

async function getFileMtime (filePath: string): Promise<Date | null> {
  try {
    const stat = await fs.stat(filePath)
    return stat.mtime
  } catch {
    return null
  }
}

interface MetaHeaders {
  etag?: string
  modified?: string
}

/**
 * Reads only the first line of the cached NDJSON metadata file to extract
 * the cache headers (etag, modified). This avoids reading and
 * parsing the full metadata (which can be megabytes for popular packages)
 * when we only need conditional-request headers.
 */
export async function loadMetaHeaders (pkgMirror: string): Promise<MetaHeaders | null> {
  let fh: fs.FileHandle | undefined
  try {
    fh = await fs.open(pkgMirror, 'r')
    // The first line (headers JSON) is typically ~100 bytes; 1 KB is plenty.
    const buf = Buffer.alloc(1024)
    const { bytesRead } = await fh.read(buf, 0, 1024, 0)
    if (bytesRead === 0) return null
    const chunk = buf.toString('utf8', 0, bytesRead)
    const newlineIdx = chunk.indexOf('\n')
    if (newlineIdx === -1) return null
    return JSON.parse(chunk.slice(0, newlineIdx)) as MetaHeaders
  } catch {
    return null
  } finally {
    await fh?.close()
  }
}

/**
 * Reads the full metadata from the cached NDJSON file.
 * Line 1: cache headers (etag, modified)
 * Line 2: registry metadata JSON
 */
export async function loadMeta (pkgMirror: string): Promise<PackageMeta | null> {
  try {
    const data = await gfs.readFile(pkgMirror, 'utf8')
    const newlineIdx = data.indexOf('\n')
    if (newlineIdx === -1) return null
    const headers = JSON.parse(data.slice(0, newlineIdx)) as MetaHeaders
    const meta = JSON.parse(data.slice(newlineIdx + 1)) as PackageMeta
    meta.etag = headers.etag
    return meta
  } catch {
    return null
  }
}

const createdDirs = new Set<string>()

export async function saveMeta (pkgMirror: string, json: string): Promise<void> {
  const dir = path.dirname(pkgMirror)
  if (!createdDirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    createdDirs.add(dir)
  }
  const temp = pathTemp(pkgMirror)
  await gfs.writeFile(temp, json, 'utf8')
  await renameOverwrite(temp, pkgMirror)
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
