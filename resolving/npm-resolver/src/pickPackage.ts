import type { MetadataCache, MetadataIndex } from '@pnpm/cache.metadata'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta, PackageMetaTime } from '@pnpm/resolving.registry.types'
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
  const manifestType = fullMetadata
    ? (ctx.filterMetadata ? 'full-filtered' : 'full')
    : 'abbreviated'
  // DB name includes registry to avoid collisions across registries
  const registryName = getRegistryHost(opts.registry)
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

  // Try to resolve from the DB index (cheap — no per-version manifest parsing)
  let cachedIndex: MetadataIndex | null | undefined
  if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
    cachedIndex = ctx.metadataDb.getIndex(dbName)
    if (cachedIndex) {
      const result = resolveFromIndex(ctx, cachedIndex, dbName, manifestType, cacheKey, spec, _pickPackageFromMeta)
      if (ctx.offline) {
        if (result) return result
        throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror for ${spec.name}`)
      }
      if (result) return result
    } else if (ctx.offline) {
      throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror for ${spec.name}`)
    }
  }

  if (!opts.updateToLatest && spec.type === 'version') {
    cachedIndex = cachedIndex ?? ctx.metadataDb.getIndex(dbName)
    if (cachedIndex) {
      const versionsMap = JSON.parse(cachedIndex.versions) as Record<string, unknown>
      if (spec.fetchSpec in versionsMap) {
        try {
          const result = resolveFromIndex(ctx, cachedIndex, dbName, manifestType, cacheKey, spec, _pickPackageFromMeta)
          if (result) return result
        } catch (err) {
          if (ctx.strictPublishedByCheck) throw err
        }
      }
    }
  }
  if (opts.publishedBy) {
    cachedIndex = cachedIndex ?? ctx.metadataDb.getIndex(dbName)
    if (cachedIndex?.cachedAt && new Date(cachedIndex.cachedAt) >= opts.publishedBy) {
      try {
        const result = resolveFromIndex(ctx, cachedIndex, dbName, manifestType, cacheKey, spec, _pickPackageFromMeta)
        if (result) return result
      } catch (err: unknown) {
        if (ctx.strictPublishedByCheck && !isMissingTimeError(err)) throw err
      }
    }
  }

  try {
    // Reuse headers from index, or do a cheap DB lookup
    let etag = cachedIndex?.etag
    let modified = cachedIndex?.modified
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

    // 304 Not Modified — registry confirmed local cache is still fresh
    if (fetchResult.notModified) {
      cachedIndex = cachedIndex ?? ctx.metadataDb.getIndex(dbName)
      if (cachedIndex) {
        const cachedAt = Date.now()
        ctx.metadataDb.updateCachedAt(dbName, cachedAt)
        const result = resolveFromIndex(ctx, { ...cachedIndex, cachedAt }, dbName, manifestType, cacheKey, spec, _pickPackageFromMeta)
        if (result) return result
      }
      throw new PnpmError('CACHE_MISSING_AFTER_304',
        `Metadata cache for ${spec.name} is unreadable after receiving 304 Not Modified`)
    }

    const cachedAt = Date.now()
    let meta = fetchResult.meta
    let typeToSave = manifestType

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
        // Save abbreviated index + manifests before re-fetching full
        if (!opts.dryRun) {
          ctx.metadataDb.queueWrite(dbName, 'abbreviated', meta, { etag: fetchResult.etag, cachedAt })
        }
        const fullFetchResult = await ctx.fetch(spec.name, {
          authHeaderValue: opts.authHeaderValue,
          fullMetadata: true,
          registry: opts.registry,
        })
        if (!fullFetchResult.notModified) {
          fetchResult = fullFetchResult
          meta = fullFetchResult.meta
          typeToSave = ctx.filterMetadata ? 'full-filtered' : 'full'
        }
      }
    }

    if (ctx.filterMetadata) {
      meta = clearMeta(meta)
    }
    meta.cachedAt = cachedAt
    meta.etag = fetchResult.etag
    ctx.metaCache.set(cacheKey, meta)
    if (!opts.dryRun) {
      ctx.metadataDb.queueWrite(dbName, typeToSave, meta, {
        etag: fetchResult.etag,
        cachedAt,
      })
    }
    return {
      meta,
      pickedPackage: _pickPackageFromMeta(meta),
    }
  } catch (err: any) { // eslint-disable-line
    err.spec = spec
    cachedIndex = cachedIndex ?? ctx.metadataDb.getIndex(dbName)
    if (!cachedIndex) throw err
    logger.error(err, err)
    logger.debug({ message: `Using cached meta from DB for ${spec.name}` })
    const result = resolveFromIndex(ctx, cachedIndex, dbName, manifestType, cacheKey, spec, _pickPackageFromMeta)
    if (result) return result
    throw err
  }
}

/**
 * Build a lightweight PackageMeta from the DB index and resolve.
 * Only parses the picked version's manifest — not all versions.
 */
function resolveFromIndex (
  ctx: { metadataDb: MetadataCache, metaCache: PackageMetaCache },
  index: MetadataIndex,
  dbName: string,
  manifestType: string,
  cacheKey: string,
  spec: RegistryPackageSpec,
  pickFn: (meta: PackageMeta) => PackageInRegistry | null
): { meta: PackageMeta, pickedPackage: PackageInRegistry | null } | null {
  const distTags = JSON.parse(index.distTags) as Record<string, string>
  const versionsCompact = JSON.parse(index.versions) as Record<string, { deprecated?: string }>
  const time = index.time ? JSON.parse(index.time) as PackageMetaTime : undefined

  // Build lightweight meta with stub version objects (just version + deprecated)
  const versions: Record<string, PackageInRegistry> = {}
  for (const [v, info] of Object.entries(versionsCompact)) {
    versions[v] = { version: v, deprecated: info.deprecated } as PackageInRegistry
  }

  const lightMeta: PackageMeta = {
    name: spec.name,
    'dist-tags': distTags,
    versions,
    time,
    modified: index.modified,
    cachedAt: index.cachedAt,
    etag: index.etag,
  }

  const pickedStub = pickFn(lightMeta)
  if (!pickedStub) return null

  // Load only the resolved version's full manifest from DB
  const manifestJson = ctx.metadataDb.getManifest(dbName, pickedStub.version, manifestType)
  if (!manifestJson) return null

  const manifest = JSON.parse(manifestJson) as PackageInRegistry
  if (lightMeta.name) {
    manifest.name = lightMeta.name
  }
  return { meta: lightMeta, pickedPackage: manifest }
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
