import type { MetadataCache, MetadataType } from '@pnpm/cache.metadata'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import getRegistryName from 'encode-registry'
import { pick } from 'ramda'
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

export interface PickPackageOptions extends PickPackageFromMetaOptions {
  authHeaderValue?: string
  pickLowestVersion?: boolean
  registry: string
  dryRun: boolean
  updateToLatest?: boolean
  optional?: boolean
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
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, fullMetadata?: boolean, etag?: string, modified?: string }) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>
    fullMetadata?: boolean
    metaCache: PackageMetaCache
    metadataDb: MetadataCache
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
  const metaType: MetadataType = fullMetadata
    ? (ctx.filterMetadata ? 'full-filtered' : 'full')
    : 'abbreviated'
  // DB name includes registry to avoid collisions across registries
  const registryName = getRegistryName(opts.registry)
  const dbName = `${registryName}/${spec.name}`
  // Cache key includes fullMetadata to avoid returning abbreviated metadata when full metadata is requested.
  const cacheKey = fullMetadata ? `${dbName}:full` : dbName
  const cachedMeta = ctx.metaCache.get(cacheKey)
  if (cachedMeta != null) {
    return {
      meta: cachedMeta,
      pickedPackage: _pickPackageFromMeta(cachedMeta),
    }
  }

  let metaCachedInStore: PackageMeta | null | undefined
  if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
    metaCachedInStore = loadMetaFromDb(ctx.metadataDb, dbName, metaType)

    if (ctx.offline) {
      if (metaCachedInStore != null) return {
        meta: metaCachedInStore,
        pickedPackage: _pickPackageFromMeta(metaCachedInStore),
      }

      throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror for ${spec.name}`)
    }

    if (metaCachedInStore != null) {
      const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
      if (pickedPackage) {
        return {
          meta: metaCachedInStore,
          pickedPackage,
        }
      }
    }
  }

  if (!opts.updateToLatest && spec.type === 'version') {
    metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(ctx.metadataDb, dbName, metaType)
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
      } catch (err) {
        if (ctx.strictPublishedByCheck) {
          throw err
        }
      }
    }
  }
  if (opts.publishedBy) {
    metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(ctx.metadataDb, dbName, metaType)
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

  try {
    // Reuse headers from already-loaded metadata, or do a cheap DB lookup
    let etag = metaCachedInStore?.etag
    let modified = metaCachedInStore?.modified ?? metaCachedInStore?.time?.modified
    if (!etag && !modified) {
      const headers = ctx.metadataDb.getHeaders(dbName, metaType)
      etag = headers?.etag
      modified = headers?.modified
    }
    let fetchResult = await ctx.fetch(spec.name, {
      authHeaderValue: opts.authHeaderValue,
      fullMetadata,
      etag,
      modified,
      registry: opts.registry,
    })

    // 304 Not Modified — registry confirmed local cache is still fresh
    if (fetchResult.notModified) {
      metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(ctx.metadataDb, dbName, metaType)
      if (metaCachedInStore != null) {
        const cachedAt = Date.now()
        metaCachedInStore.cachedAt = cachedAt
        ctx.metaCache.set(cacheKey, metaCachedInStore)
        ctx.metadataDb.updateCachedAt(dbName, metaType, cachedAt)
        return {
          meta: metaCachedInStore,
          pickedPackage: _pickPackageFromMeta(metaCachedInStore),
        }
      }
      throw new PnpmError('CACHE_MISSING_AFTER_304',
        `Metadata cache for ${spec.name} is unreadable after receiving 304 Not Modified`)
    }

    const cachedAt = Date.now()
    let meta = fetchResult.meta
    let jsonToSave: string | undefined
    let metaTypeToSave: MetadataType = metaType

    // When minimumReleaseAge is active and we fetched abbreviated metadata,
    // check if the package was recently modified and needs full metadata
    // for per-version time-based filtering.
    if (
      opts.publishedBy &&
      !fullMetadata &&
      meta.time == null &&
      opts.publishedByExclude?.(spec.name) !== true
    ) {
      const modifiedDate = meta.modified ? new Date(meta.modified) : null
      const isModifiedValid = modifiedDate != null && !Number.isNaN(modifiedDate.getTime())
      if (!isModifiedValid || modifiedDate >= opts.publishedBy) {
        // Save the abbreviated metadata to the DB before re-fetching full.
        if (!opts.dryRun) {
          const abbreviatedData = typeof fetchResult.jsonText === 'string' ? fetchResult.jsonText : JSON.stringify(meta)
          try {
            ctx.metadataDb.set(dbName, 'abbreviated', abbreviatedData, {
              etag: fetchResult.etag,
              modified: meta.modified ?? meta.time?.modified,
              cachedAt,
            })
          } catch {
            // We don't care if this was not written to the cache
          }
        }
        const fullFetchResult = await ctx.fetch(spec.name, {
          authHeaderValue: opts.authHeaderValue,
          fullMetadata: true,
          registry: opts.registry,
        })
        if (!fullFetchResult.notModified) {
          fetchResult = fullFetchResult
          meta = fullFetchResult.meta
          metaTypeToSave = ctx.filterMetadata ? 'full-filtered' : 'full'
        }
      }
    }

    if (ctx.filterMetadata) {
      meta = clearMeta(meta)
      jsonToSave = undefined
    } else if (typeof fetchResult.jsonText === 'string') {
      jsonToSave = fetchResult.jsonText
    }
    meta.cachedAt = cachedAt
    meta.etag = fetchResult.etag
    // only save meta to cache, when it is fresh
    ctx.metaCache.set(cacheKey, meta)
    if (!opts.dryRun) {
      const dataForDb = jsonToSave ?? JSON.stringify(meta)
      try {
        ctx.metadataDb.set(dbName, metaTypeToSave, dataForDb, {
          etag: fetchResult.etag,
          modified: meta.modified ?? meta.time?.modified,
          cachedAt,
        })
      } catch {
        // We don't care if this was not written to the cache
      }
    }
    return {
      meta,
      pickedPackage: _pickPackageFromMeta(meta),
    }
  } catch (err: any) { // eslint-disable-line
    err.spec = spec
    const meta = loadMetaFromDb(ctx.metadataDb, dbName, metaType)
    if (meta == null) throw err
    logger.error(err, err)
    logger.debug({ message: `Using cached meta from DB for ${spec.name}` })
    return {
      meta,
      pickedPackage: _pickPackageFromMeta(meta),
    }
  }
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

function loadMetaFromDb (db: MetadataCache, name: string, type: MetadataType): PackageMeta | null {
  const row = db.get(name, type)
  if (!row) return null
  const meta = JSON.parse(row.data) as PackageMeta
  meta.cachedAt = row.cachedAt
  meta.etag = row.etag
  return meta
}

function isMissingTimeError (err: unknown): boolean {
  return (
    err != null &&
    typeof err === 'object' &&
    'code' in err &&
    (err as { code: string }).code === 'ERR_PNPM_MISSING_TIME'
  )
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
