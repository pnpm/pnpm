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
import { pick } from 'ramda'
import { renameOverwrite } from 'rename-overwrite'
import semver from 'semver'

import type { FetchMetadataNotModifiedResult, FetchMetadataResult } from './fetch.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import {
  pickLowestVersionByVersionRange,
  pickPackageFromMeta,
  type PickPackageFromMetaOptions,
  pickVersionByVersionRange,
} from './pickPackageFromMeta.js'
import { toRaw } from './toRaw.js'

export interface PackageMetaCache {
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
}

interface PickerOptions extends PickPackageFromMetaOptions {
  pickLowestVersion?: boolean
  includeLatestTag?: boolean
  strictPublishedByCheck?: boolean
  ignoreMissingTimeField?: boolean
}

// When includeLatestTag is set, the "latest" dist-tag is added as a candidate
// alongside the requested spec, and the higher-versioned pick wins.
function runPicker (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  pickOne: (targetSpec: RegistryPackageSpec) => PackageInRegistry | null
): PackageInRegistry | null {
  const currentPkg = pickOne(spec)
  if (!pickerOpts.includeLatestTag) return currentPkg
  const latestPkg = pickOne({ ...spec, type: 'tag', fetchSpec: 'latest' })
  return pickMax(latestPkg, currentPkg)
}

// Returns whichever pick has the higher version, treating null as "no match".
function pickMax (
  a: PackageInRegistry | null,
  b: PackageInRegistry | null
): PackageInRegistry | null {
  if (!a) return b
  if (!b) return a
  return semver.lt(a.version, b.version) ? b : a
}

const pickHighest = pickPackageFromMeta.bind(null, pickVersionByVersionRange)
const pickLowest = pickPackageFromMeta.bind(null, pickLowestVersionByVersionRange)

// When minimumReleaseAge is active: try the highest mature version; if none
// and strictPublishedByCheck is off, fall back to the lowest version in range
// without applying the maturity filter.
function pickRespectingMinReleaseAge (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  return runPicker(pickerOpts, spec, (targetSpec) => {
    const highest = pickHighest(pickerOpts, meta, targetSpec)
    if (highest || pickerOpts.strictPublishedByCheck) return highest
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
): PackageInRegistry | null {
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
): PackageInRegistry | null {
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
): PackageInRegistry | null {
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

export async function pickPackage (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, fullMetadata?: boolean, etag?: string, modified?: string }) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>
    fullMetadata?: boolean
    metaCache: PackageMetaCache
    cacheDir: string
    offline?: boolean
    preferOffline?: boolean
    filterMetadata?: boolean
    strictPublishedByCheck?: boolean
    ignoreMissingTimeField?: boolean
  },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null }> {
  opts = opts || {}

  const pickerOpts: PickerOptions = {
    preferredVersionSelectors: opts.preferredVersionSelectors,
    publishedBy: opts.publishedBy,
    publishedByExclude: opts.publishedByExclude,
    pickLowestVersion: opts.pickLowestVersion,
    includeLatestTag: opts.includeLatestTag,
    strictPublishedByCheck: ctx.strictPublishedByCheck,
    ignoreMissingTimeField: ctx.ignoreMissingTimeField,
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
  const cachedMeta = ctx.metaCache.get(cacheKey)
  if (cachedMeta != null) {
    return {
      meta: cachedMeta,
      pickedPackage: pickMatchingVersionFinal(pickerOpts, spec, cachedMeta),
    }
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(ctx.cacheDir, metaDir, registryName, `${encodePkgName(spec.name)}.jsonl`)

  return runLimited(pkgMirror, async (limit) => {
    let metaCachedInStore: PackageMeta | null | undefined
    if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
      metaCachedInStore = await limit(async () => loadMeta(pkgMirror))

      if (ctx.offline) {
        if (metaCachedInStore != null) return {
          meta: metaCachedInStore,
          pickedPackage: pickMatchingVersionFinal(pickerOpts, spec, metaCachedInStore),
        }

        throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
      }

      if (metaCachedInStore != null) {
        const pickedPackage = pickMatchingVersionFinal(pickerOpts, spec, metaCachedInStore)
        if (pickedPackage) {
          return {
            meta: metaCachedInStore,
            pickedPackage,
          }
        }
      }
    }

    if (!opts.includeLatestTag && spec.type === 'version') {
      metaCachedInStore = metaCachedInStore ?? await limit(async () => loadMeta(pkgMirror))
      // use the cached meta only if it has the required package version
      // otherwise it is probably out of date
      if ((metaCachedInStore?.versions?.[spec.fetchSpec]) != null) {
        try {
          const pickedPackage = pickMatchingVersionFast(pickerOpts, spec, metaCachedInStore)
          if (pickedPackage) {
            return {
              meta: metaCachedInStore,
              pickedPackage,
            }
          }
        } catch (err) {
          if (ctx.strictPublishedByCheck) {
            throw err
          }
        }
      }
    }
    if (opts.publishedBy) {
      const mtime = await limit(async () => getFileMtime(pkgMirror))
      if (mtime != null && mtime >= opts.publishedBy) {
        metaCachedInStore = metaCachedInStore ?? await limit(async () => loadMeta(pkgMirror))
        if (metaCachedInStore != null) {
          try {
            const pickedPackage = pickMatchingVersionFast(pickerOpts, spec, metaCachedInStore)
            if (pickedPackage) {
              return {
                meta: metaCachedInStore,
                pickedPackage,
              }
            }
          } catch (err: unknown) {
            // Don't rethrow ERR_PNPM_MISSING_TIME from cached abbreviated metadata —
            // let the code fall through to the network fetch path which will get full metadata.
            if (
              ctx.strictPublishedByCheck &&
              !(isMissingTimeError(err))
            ) {
              throw err
            }
          }
        }
      }
    }

    try {
      // Load only the cache headers (etag, modified) for conditional request headers.
      // This avoids reading and parsing the full metadata file (which can be megabytes)
      // when the registry returns 200 and the old metadata would be discarded anyway.
      const cacheHeaders = metaCachedInStore != null
        ? { etag: metaCachedInStore.etag, modified: metaCachedInStore.modified ?? metaCachedInStore.time?.modified }
        : await limit(async () => loadMetaHeaders(pkgMirror))
      let fetchResult = await ctx.fetch(spec.name, {
        authHeaderValue: opts.authHeaderValue,
        fullMetadata,
        etag: cacheHeaders?.etag,
        modified: cacheHeaders?.modified,
        registry: opts.registry,
      })

      // 304 Not Modified — registry confirmed local cache is still fresh.
      // Now we need the full metadata, so load it from disk.
      if (fetchResult.notModified) {
        metaCachedInStore = metaCachedInStore ?? await limit(async () => loadMeta(pkgMirror))
        if (metaCachedInStore != null) {
          ctx.metaCache.set(cacheKey, metaCachedInStore)
          return {
            meta: metaCachedInStore,
            pickedPackage: pickMatchingVersionFinal(pickerOpts, spec, metaCachedInStore),
          }
        }
        throw new PnpmError('CACHE_MISSING_AFTER_304',
          `Metadata cache for ${spec.name} is unreadable after receiving 304 Not Modified`)
      }

      let meta = fetchResult.meta
      let resultToSave: FetchMetadataResult = fetchResult

      // When minimumReleaseAge is active and we fetched abbreviated metadata,
      // check if the package was recently modified and needs full metadata
      // for per-version time-based filtering.
      //
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
        if (!isModifiedValid || modifiedDate >= opts.publishedBy) {
          // Save the abbreviated metadata to the abbreviated cache before re-fetching full.
          if (!opts.dryRun) {
            const abbreviatedJson = prepareJsonForDisk(fetchResult.meta, fetchResult.etag, fetchResult.jsonText)
            // Fire-and-forget save to the abbreviated cache path (pkgMirror).
            runLimited(pkgMirror, (limit) => limit(async () => {
              try {
                await saveMeta(pkgMirror, abbreviatedJson)
              } catch (err: any) { // eslint-disable-line
                // We don't care if this file was not written to the cache
              }
            }))
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

      if (ctx.filterMetadata) {
        meta = clearMeta(meta)
      }
      if (!opts.dryRun) {
        // Serialize before setting meta.etag so it only lives in the headers line, not the body.
        const jsonForDisk = ctx.filterMetadata
          ? prepareJsonForDisk(meta, resultToSave.etag)
          : prepareJsonForDisk(resultToSave.meta, resultToSave.etag, resultToSave.jsonText)
        runLimited(pkgMirror, (limit) => limit(async () => {
          try {
            await saveMeta(pkgMirror, jsonForDisk)
          } catch (err: any) { // eslint-disable-line
            // We don't care if this file was not written to the cache
          }
        }))
      }
      meta.etag = resultToSave.etag
      // only save meta to cache, when it is fresh
      ctx.metaCache.set(cacheKey, meta)
      return {
        meta,
        pickedPackage: pickMatchingVersionFinal(pickerOpts, spec, meta),
      }
    } catch (err: any) { // eslint-disable-line
      err.spec = spec
      const meta = await loadMeta(pkgMirror) // TODO: add test for this usecase
      if (meta == null) throw err
      logger.error(err, err)
      logger.debug({ message: `Using cached meta from ${pkgMirror}` })
      return {
        meta,
        pickedPackage: pickMatchingVersionFinal(pickerOpts, spec, meta),
      }
    }
  })
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
    modified: pkg.modified,
  }
}

function encodePkgName (pkgName: string): string {
  if (pkgName !== pkgName.toLowerCase()) {
    return `${pkgName}_${createHexHash(pkgName)}`
  }
  return pkgName
}

/**
 * Formats metadata for disk storage as two-line NDJSON:
 *   Line 1: cache headers (etag, modified) — small, fast to read
 *   Line 2: the full registry metadata JSON — unchanged from the registry response
 */
function prepareJsonForDisk (meta: PackageMeta, etag: string | undefined, jsonText?: string): string {
  const modified = meta.modified ?? meta.time?.modified
  const headers = JSON.stringify({ etag, modified })
  const body = jsonText ?? JSON.stringify(meta)
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

function warnMissingTimeFieldOnce (pkgName: string): void {
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
async function loadMetaHeaders (pkgMirror: string): Promise<MetaHeaders | null> {
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
async function loadMeta (pkgMirror: string): Promise<PackageMeta | null> {
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

async function saveMeta (pkgMirror: string, json: string): Promise<void> {
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
