import { promises as fs } from 'fs'
import path from 'path'
import util from 'util'
import { ABBREVIATED_META_DIR, FULL_META_DIR, FULL_FILTERED_META_DIR } from '@pnpm/constants'
import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import gfs from '@pnpm/graceful-fs'
import { type PackageMeta, type PackageInRegistry } from '@pnpm/registry.types'
import getRegistryName from 'encode-registry'
import loadJsonFile from 'load-json-file'
import pLimit from 'p-limit'
import { fastPathTemp as pathTemp } from 'path-temp'
import pick from 'ramda/src/pick'
import semver from 'semver'
import renameOverwrite from 'rename-overwrite'
import { toRaw } from './toRaw.js'
import {
  pickPackageFromMeta,
  pickVersionByVersionRange,
  pickLowestVersionByVersionRange,
  type PickPackageFromMetaOptions,
} from './pickPackageFromMeta.js'
import { type RegistryPackageSpec } from './parseBareSpecifier.js'

export interface PackageMetaCache {
  get: (key: string) => PackageMeta | undefined
  set: (key: string, meta: PackageMeta) => void
  has: (key: string) => boolean
}

interface RefCountedLimiter {
  count: number
  limit: pLimit.Limit
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
async function runLimited<T> (pkgMirror: string, fn: (limit: pLimit.Limit) => Promise<T>): Promise<T> {
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
  updateToLatest?: boolean
  optional?: boolean
  /**
   * When true, skip the on-disk exact-version cache fast path so a
   * stale on-disk packument can't satisfy the call without a
   * conditional registry request. The in-memory cache is left alone:
   * its entries can only be populated by this install's own fresh
   * network fetches, so they're authoritative for second-and-onward
   * lookups within the same install.
   */
  updateChecksums?: boolean
}

const pickPackageFromMetaUsingTimeStrict = pickPackageFromMeta.bind(null, pickVersionByVersionRange)

function pickPackageFromMetaUsingTime (
  opts: PickPackageFromMetaOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  const pickedPackage = pickPackageFromMeta(pickVersionByVersionRange, opts, spec, meta)
  if (pickedPackage) return pickedPackage
  return pickPackageFromMeta(pickLowestVersionByVersionRange, {
    preferredVersionSelectors: opts.preferredVersionSelectors,
  }, spec, meta)
}

export async function pickPackage (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, fullMetadata?: boolean }) => Promise<PackageMeta>
    fullMetadata?: boolean
    metaCache: PackageMetaCache
    cacheDir: string
    offline?: boolean
    preferOffline?: boolean
    filterMetadata?: boolean
    strictPublishedByCheck?: boolean
  },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null }> {
  opts = opts || {}
  const pickPackageFromMetaBySpec = (
    opts.publishedBy
      ? (ctx.strictPublishedByCheck ? pickPackageFromMetaUsingTimeStrict : pickPackageFromMetaUsingTime)
      : (pickPackageFromMeta.bind(null, opts.pickLowestVersion ? pickLowestVersionByVersionRange : pickVersionByVersionRange))
  ).bind(null, {
    preferredVersionSelectors: opts.preferredVersionSelectors,
    publishedBy: opts.publishedBy,
    publishedByExclude: opts.publishedByExclude,
  })

  let _pickPackageFromMeta!: (meta: PackageMeta) => PackageInRegistry | null
  if (opts.updateToLatest) {
    _pickPackageFromMeta = (meta) => {
      const latestStableSpec: RegistryPackageSpec = { ...spec, type: 'tag', fetchSpec: 'latest' }
      const latestStable = pickPackageFromMetaBySpec(latestStableSpec, meta)
      const current = pickPackageFromMetaBySpec(spec, meta)

      if (!latestStable) return current
      if (!current) return latestStable
      if (semver.lt(latestStable.version, current.version)) return current
      return latestStable
    }
  } else {
    _pickPackageFromMeta = pickPackageFromMetaBySpec.bind(null, spec)
  }

  validatePackageName(spec.name)

  // Use full metadata for optional dependencies to get libc field.
  // See: https://github.com/pnpm/pnpm/issues/9950
  const fullMetadata = opts.optional === true || ctx.fullMetadata === true
  const metaDir = fullMetadata
    ? (ctx.filterMetadata ? FULL_FILTERED_META_DIR : FULL_META_DIR)
    : ABBREVIATED_META_DIR
  // Cache key includes fullMetadata to avoid returning abbreviated metadata when full metadata is requested.
  const cacheKey = fullMetadata ? `${spec.name}:full` : spec.name
  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(ctx.cacheDir, metaDir, registryName, `${encodePkgName(spec.name)}.json`)
  const cachedMeta = ctx.metaCache.get(cacheKey)
  if (cachedMeta != null) {
    // The in-memory cache may hold abbreviated metadata from an earlier call
    // that didn't need `time` (no publishedBy then). If this call has
    // publishedBy, upgrade to full metadata so the maturity check can run on
    // real time data instead of throwing ERR_PNPM_MISSING_TIME.
    const upgraded = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, cachedMeta)
    let metaForCache = upgraded.meta
    if (upgraded.upgraded) {
      metaForCache = persistUpgradedMeta(ctx, pkgMirror, metaForCache, opts.dryRun)
      ctx.metaCache.set(cacheKey, metaForCache)
    }
    return {
      meta: metaForCache,
      pickedPackage: _pickPackageFromMeta(metaForCache),
    }
  }

  return runLimited(pkgMirror, async (limit) => {
    let metaCachedInStore: PackageMeta | null | undefined
    if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
      metaCachedInStore = await limit(async () => loadMeta(pkgMirror))

      if (ctx.offline) {
        if (metaCachedInStore != null) return {
          meta: metaCachedInStore,
          pickedPackage: _pickPackageFromMeta(metaCachedInStore),
        }

        throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
      }

      if (metaCachedInStore != null) {
        // Disk-cached meta may be abbreviated; upgrade for the maturity check
        // instead of letting the picker throw ERR_PNPM_MISSING_TIME.
        const upgraded = await maybeUpgradeAbbreviatedMetaForReleaseAge(ctx, spec, opts, metaCachedInStore)
        metaCachedInStore = upgraded.meta
        if (upgraded.upgraded) {
          metaCachedInStore = persistUpgradedMeta(ctx, pkgMirror, metaCachedInStore, opts.dryRun)
          ctx.metaCache.set(cacheKey, metaCachedInStore)
        }
        const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
        if (pickedPackage) {
          return {
            meta: metaCachedInStore,
            pickedPackage,
          }
        }
      }
    }

    if (!opts.updateToLatest && !opts.updateChecksums && spec.type === 'version') {
      metaCachedInStore = metaCachedInStore ?? await limit(async () => loadMeta(pkgMirror))
      // use the cached meta only if it has the required package version
      // otherwise it is probably out of date
      if ((metaCachedInStore?.versions?.[spec.fetchSpec]) != null) {
        try {
          const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
          if (pickedPackage) {
            return {
              meta: metaCachedInStore,
              pickedPackage,
            }
          }
        } catch (err: unknown) {
          // MISSING_TIME from cached abbreviated metadata should fall through
          // to the network fetch path even under strictPublishedByCheck —
          // the fetch will upgrade to full metadata and run the maturity check
          // on real `time` data.
          if (shouldRethrowFromFastPathCache(err, ctx.strictPublishedByCheck)) {
            throw err
          }
        }
      }
    }
    if (opts.publishedBy) {
      metaCachedInStore = metaCachedInStore ?? await limit(async () => loadMeta(pkgMirror))
      if (metaCachedInStore?.cachedAt && new Date(metaCachedInStore.cachedAt) >= opts.publishedBy) {
        try {
          const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
          if (pickedPackage) {
            return {
              meta: metaCachedInStore,
              pickedPackage,
            }
          }
        } catch (err: unknown) {
          if (shouldRethrowFromFastPathCache(err, ctx.strictPublishedByCheck)) {
            throw err
          }
        }
      }
    }

    try {
      let meta = await ctx.fetch(spec.name, {
        authHeaderValue: opts.authHeaderValue,
        fullMetadata,
        registry: opts.registry,
      })
      // When publishedBy is active but the registry returned abbreviated
      // metadata (no per-version `time`), re-fetch with `fullMetadata: true`
      // so the maturity check can run properly. Without this, abbreviated
      // metadata + publishedBy would throw ERR_PNPM_MISSING_TIME.
      if (
        opts.publishedBy &&
        !fullMetadata &&
        meta.time == null &&
        opts.publishedByExclude?.(spec.name) !== true
      ) {
        meta = await ctx.fetch(spec.name, {
          authHeaderValue: opts.authHeaderValue,
          fullMetadata: true,
          registry: opts.registry,
        })
      }
      if (ctx.filterMetadata) {
        meta = clearMeta(meta)
      }
      meta.cachedAt = Date.now()
      // only save meta to cache, when it is fresh
      ctx.metaCache.set(cacheKey, meta)
      if (!opts.dryRun) {
        // We stringify this meta here to avoid saving any mutations that could happen to the meta object.
        const stringifiedMeta = JSON.stringify(meta)
        // eslint-disable-next-line @typescript-eslint/no-floating-promises
        runLimited(pkgMirror, (limit) => limit(async () => {
          try {
            await saveMeta(pkgMirror, stringifiedMeta)
          } catch (err: any) { // eslint-disable-line
            // We don't care if this file was not written to the cache
          }
        }))
      }
      return {
        meta,
        pickedPackage: _pickPackageFromMeta(meta),
      }
    } catch (err: any) { // eslint-disable-line
      err.spec = spec
      const meta = await loadMeta(pkgMirror) // TODO: add test for this usecase
      if (meta == null) throw err
      logger.error(err, err)
      logger.debug({ message: `Using cached meta from ${pkgMirror}` })
      return {
        meta,
        pickedPackage: _pickPackageFromMeta(meta),
      }
    }
  })
}

// When `publishedBy` is active and the cached metadata is abbreviated (no
// per-version `time`), the maturity check can't run on the data we have and
// `pickPackageFromMeta` will throw ERR_PNPM_MISSING_TIME. Upgrade to full
// metadata via a follow-up fetch so the check can proceed on real `time` data.
async function maybeUpgradeAbbreviatedMetaForReleaseAge (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, fullMetadata?: boolean }) => Promise<PackageMeta>
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
): Promise<{ meta: PackageMeta, upgraded: boolean }> {
  if (
    ctx.offline === true ||
    !opts.publishedBy ||
    meta.time != null ||
    opts.publishedByExclude?.(spec.name) === true
  ) {
    return { meta, upgraded: false }
  }
  const fullMeta = await ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    fullMetadata: true,
    registry: opts.registry,
  })
  return { meta: fullMeta, upgraded: true }
}

// Returns true when a fast-path cache catch should rethrow. MISSING_TIME is
// excluded so callers fall through to the network fetch path, which can
// upgrade abbreviated cached metadata to full and run the maturity check on
// real `time` data.
function shouldRethrowFromFastPathCache (err: unknown, strictPublishedByCheck: boolean | undefined): boolean {
  if (isMissingTimeError(err)) return false
  return strictPublishedByCheck === true
}

function isMissingTimeError (err: unknown): boolean {
  return util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_MISSING_TIME'
}

// Persists upgraded full metadata to the on-disk cache mirror and returns the
// meta to store in the in-memory cache. When `filterMetadata` is on the
// returned meta is stripped via `clearMeta`. Without persisting here, a fresh
// process would re-trigger the upgrade fetch on its next install since the
// on-disk cache still holds the abbreviated form.
function persistUpgradedMeta (
  ctx: { filterMetadata?: boolean },
  pkgMirror: string,
  meta: PackageMeta,
  dryRun: boolean
): PackageMeta {
  const metaForCache = ctx.filterMetadata ? clearMeta(meta) : meta
  metaForCache.cachedAt = Date.now()
  if (!dryRun) {
    const stringifiedMeta = JSON.stringify(metaForCache)
    // eslint-disable-next-line @typescript-eslint/no-floating-promises
    runLimited(pkgMirror, (l) => l(async () => {
      try {
        await saveMeta(pkgMirror, stringifiedMeta)
      } catch (err: any) { // eslint-disable-line
        // We don't care if this file was not written to the cache
      }
    }))
  }
  return metaForCache
}

function clearMeta (pkg: PackageMeta): PackageMeta {
  const versions: PackageMeta['versions'] = {}
  for (const [version, info] of Object.entries(pkg.versions)) {
    // The list taken from https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object
    // with the addition of 'libc'
    versions[version] = pick([
      'name',
      'version',
      'bin',
      'directories',
      'devDependencies',
      'optionalDependencies',
      'dependencies',
      'peerDependencies',
      'dist',
      'engines',
      'peerDependenciesMeta',
      'cpu',
      'os',
      'libc',
      'deprecated',
      'bundleDependencies',
      'bundledDependencies',
      'hasInstallScript',
      '_npmUser',
    ], info)
  }

  return {
    name: pkg.name,
    'dist-tags': pkg['dist-tags'],
    versions,
    time: pkg.time,
    cachedAt: pkg.cachedAt,
  }
}

function encodePkgName (pkgName: string): string {
  if (pkgName !== pkgName.toLowerCase()) {
    return `${pkgName}_${createHexHash(pkgName)}`
  }
  return pkgName
}

async function loadMeta (pkgMirror: string): Promise<PackageMeta | null> {
  try {
    return await loadJsonFile<PackageMeta>(pkgMirror)
  } catch (err: any) { // eslint-disable-line
    return null
  }
}

const createdDirs = new Set<string>()

async function saveMeta (pkgMirror: string, meta: string): Promise<void> {
  const dir = path.dirname(pkgMirror)
  if (!createdDirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    createdDirs.add(dir)
  }
  const temp = pathTemp(pkgMirror)
  await gfs.writeFile(temp, meta)
  await renameOverwrite(temp, pkgMirror)
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
