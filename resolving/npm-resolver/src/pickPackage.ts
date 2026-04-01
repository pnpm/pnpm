import type { MetadataCache } from '@pnpm/cache.metadata'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
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

  const fullMetadata = opts.optional === true || ctx.fullMetadata === true
  const registryName = getRegistryHost(opts.registry)
  const dbName = `${registryName}/${spec.name}`

  let metaCachedInStore: PackageMeta | null | undefined
  if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
    metaCachedInStore = loadMetaFromDb(ctx.metadataDb, dbName, fullMetadata)

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
        return { meta: metaCachedInStore, pickedPackage }
      }
    }
  }

  if (!opts.updateToLatest && spec.type === 'version') {
    metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(ctx.metadataDb, dbName, fullMetadata)
    if ((metaCachedInStore?.versions?.[spec.fetchSpec]) != null) {
      try {
        const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
        if (pickedPackage) {
          return { meta: metaCachedInStore, pickedPackage }
        }
      } catch (err) {
        if (ctx.strictPublishedByCheck) throw err
      }
    }
  }
  if (opts.publishedBy) {
    metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(ctx.metadataDb, dbName, fullMetadata)
    if (metaCachedInStore?.cachedAt && new Date(metaCachedInStore.cachedAt) >= opts.publishedBy) {
      try {
        const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
        if (pickedPackage) {
          return { meta: metaCachedInStore, pickedPackage }
        }
      } catch (err: unknown) {
        if (ctx.strictPublishedByCheck && !isMissingTimeError(err)) throw err
      }
    }
  }

  try {
    let etag = metaCachedInStore?.etag
    let modified = metaCachedInStore?.modified ?? metaCachedInStore?.time?.modified
    if (!etag || !modified) {
      const headers = ctx.metadataDb.getHeaders(dbName)
      etag = etag ?? headers?.etag
      modified = modified ?? headers?.modified
    }
    let fetchResult = await ctx.fetch(spec.name, {
      authHeaderValue: opts.authHeaderValue,
      fullMetadata,
      etag,
      modified,
      registry: opts.registry,
    })

    // 304 Not Modified — trust whatever is cached, the registry just validated it
    if (fetchResult.notModified) {
      metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(ctx.metadataDb, dbName, false)
      if (metaCachedInStore != null) {
        const cachedAt = Date.now()
        metaCachedInStore.cachedAt = cachedAt
        ctx.metadataDb.updateCachedAt(dbName, cachedAt)
        return { meta: metaCachedInStore, pickedPackage: _pickPackageFromMeta(metaCachedInStore) }
      }
      throw new PnpmError('CACHE_MISSING_AFTER_304',
        `Metadata cache for ${spec.name} is unreadable after receiving 304 Not Modified`)
    }

    const cachedAt = Date.now()
    let meta = fetchResult.meta

    // When minimumReleaseAge is active and we fetched abbreviated metadata,
    // check if the package was recently modified and needs full metadata.
    if (
      opts.publishedBy &&
      !fullMetadata &&
      meta.time == null &&
      opts.publishedByExclude?.(spec.name) !== true
    ) {
      const modifiedDate = meta.modified ? new Date(meta.modified) : null
      const isModifiedValid = modifiedDate != null && !Number.isNaN(modifiedDate.getTime())
      if (!isModifiedValid || modifiedDate >= opts.publishedBy) {
        // Save abbreviated metadata before re-fetching full
        if (!opts.dryRun) {
          const abbreviatedJson = typeof fetchResult.jsonText === 'string' ? fetchResult.jsonText : JSON.stringify(meta)
          ctx.metadataDb.queueSet(dbName, abbreviatedJson, {
            etag: fetchResult.etag,
            modified: meta.modified,
            cachedAt,
          })
        }
        const fullFetchResult = await ctx.fetch(spec.name, {
          authHeaderValue: opts.authHeaderValue,
          fullMetadata: true,
          registry: opts.registry,
        })
        if (!fullFetchResult.notModified) {
          fetchResult = fullFetchResult
          meta = fullFetchResult.meta
        }
      }
    }

    if (ctx.filterMetadata) {
      meta = clearMeta(meta)
    }
    meta.cachedAt = cachedAt
    meta.etag = fetchResult.etag
    if (!opts.dryRun) {
      const rawJson = (ctx.filterMetadata || typeof fetchResult.jsonText !== 'string')
        ? JSON.stringify(meta)
        : fetchResult.jsonText
      ctx.metadataDb.queueSet(dbName, rawJson, {
        etag: fetchResult.etag,
        modified: meta.modified ?? meta.time?.modified,
        cachedAt,
        isFull: fullMetadata,
      })
    }
    return { meta, pickedPackage: _pickPackageFromMeta(meta) }
  } catch (err: any) { // eslint-disable-line
    err.spec = spec
    const meta = loadMetaFromDb(ctx.metadataDb, dbName, fullMetadata)
    if (meta == null) throw err
    logger.error(err, err)
    logger.debug({ message: `Using cached meta from DB for ${spec.name}` })
    return { meta, pickedPackage: _pickPackageFromMeta(meta) }
  }
}

function clearMeta (pkg: PackageMeta): PackageMeta {
  const versions: PackageMeta['versions'] = {}
  for (const [version, info] of Object.entries(pkg.versions)) {
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

function loadMetaFromDb (db: MetadataCache, name: string, needsFull: boolean): PackageMeta | null {
  const row = db.get(name)
  if (!row) return null
  if (needsFull && !row.isFull) return null
  const meta = JSON.parse(row.data) as PackageMeta
  meta.cachedAt = row.cachedAt
  meta.etag = row.etag
  meta.modified = row.modified ?? meta.modified
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

const registryHostCache = new Map<string, string>()

function getRegistryHost (registry: string): string {
  let host = registryHostCache.get(registry)
  if (host == null) {
    host = new URL(registry).host
    registryHostCache.set(registry, host)
  }
  return host
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
