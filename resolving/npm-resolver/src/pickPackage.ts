import { logger } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import {
  resolveMetadataDataInWorker,
  resolveMetadataInWorker,
  type ResolveMetadataSpec,
} from '@pnpm/worker'

import type { FetchMetadataNotModifiedResult, FetchMetadataResult } from './fetch.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import type { PickPackageFromMetaOptions } from './pickPackageFromMeta.js'

export interface PickPackageOptions extends PickPackageFromMetaOptions {
  authHeaderValue?: string
  pickLowestVersion?: boolean
  registry: string
  dryRun: boolean
  updateToLatest?: boolean
  optional?: boolean
}

export async function pickPackage (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, fullMetadata?: boolean, etag?: string, modified?: string }) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>
    fullMetadata?: boolean
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

  const fullMetadata = opts.optional === true || ctx.fullMetadata === true

  // Pre-compute publishedByExclude result on main thread (function can't be serialized)
  const publishedByExcludeResult = opts.publishedBy && opts.publishedByExclude
    ? opts.publishedByExclude(spec.name)
    : undefined

  const commonOpts = {
    cacheDir: ctx.cacheDir,
    spec: spec as ResolveMetadataSpec,
    registry: opts.registry,
    offline: ctx.offline,
    preferOffline: ctx.preferOffline,
    pickLowestVersion: opts.pickLowestVersion,
    updateToLatest: opts.updateToLatest,
    fullMetadata,
    filterMetadata: ctx.filterMetadata,
    strictPublishedByCheck: ctx.strictPublishedByCheck,
    dryRun: opts.dryRun,
    publishedBy: opts.publishedBy?.getTime(),
    publishedByExcludeResult,
    preferredVersionSelectors: opts.preferredVersionSelectors,
  }

  // Round 1: check cache in worker
  try {
    const round1 = await resolveMetadataInWorker(commonOpts)

    if (round1.cacheHit) {
      return {
        meta: round1.meta as unknown as PackageMeta,
        pickedPackage: round1.pickedPackage as unknown as PackageInRegistry | null,
      }
    }

    // Cache miss - do network fetch on main thread
    const fetchResult = await ctx.fetch(spec.name, {
      authHeaderValue: opts.authHeaderValue,
      fullMetadata,
      etag: round1.etag,
      modified: round1.modified,
      registry: opts.registry,
    })

    // 304 Not Modified - let worker re-read from cache and update cachedAt
    if (fetchResult.notModified) {
      const result = await resolveMetadataDataInWorker({
        ...commonOpts,
        jsonText: '',
        notModified: true,
      })
      if (!result.needsFullRefetch) {
        return {
          meta: result.meta as unknown as PackageMeta,
          pickedPackage: result.pickedPackage as unknown as PackageInRegistry | null,
        }
      }
      // Defensive: if 304 handler asks for full refetch, do it
      return fetchFullAndProcess(ctx, spec, opts, commonOpts)
    }

    // Round 2: send fetched data to worker for processing
    const round2 = await resolveMetadataDataInWorker({
      ...commonOpts,
      jsonText: fetchResult.jsonText,
      etag: fetchResult.etag,
    })

    // If abbreviated metadata needs full re-fetch
    if (round2.needsFullRefetch) {
      return fetchFullAndProcess(ctx, spec, opts, commonOpts)
    }

    return {
      meta: round2.meta as unknown as PackageMeta,
      pickedPackage: round2.pickedPackage as unknown as PackageInRegistry | null,
    }
  } catch (err: any) { // eslint-disable-line
    err.spec = spec
    // On network error, try to fall back to cached data via the worker
    try {
      const fallback = await resolveMetadataInWorker({
        ...commonOpts,
        // Force offline mode to get whatever is cached
        offline: true,
      })
      if (fallback.cacheHit) {
        logger.error(err, err)
        logger.debug({ message: `Using cached meta from DB for ${spec.name}` })
        return {
          meta: fallback.meta as unknown as PackageMeta,
          pickedPackage: fallback.pickedPackage as unknown as PackageInRegistry | null,
        }
      }
    } catch {
      // no cached data available, throw original error
    }
    throw err
  }
}

async function fetchFullAndProcess (
  ctx: {
    fetch: (pkgName: string, opts: { registry: string, authHeaderValue?: string, fullMetadata?: boolean, etag?: string, modified?: string }) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>
  },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions,
  commonOpts: Parameters<typeof resolveMetadataDataInWorker>[0] extends infer T ? Omit<T, 'jsonText' | 'etag' | 'notModified'> : never
): Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null }> {
  const fullFetchResult = await ctx.fetch(spec.name, {
    authHeaderValue: opts.authHeaderValue,
    fullMetadata: true,
    registry: opts.registry,
  })
  if (fullFetchResult.notModified) {
    // Very unusual: full re-fetch also returned 304
    const fallback = await resolveMetadataDataInWorker({
      ...commonOpts,
      jsonText: '',
      notModified: true,
      fullMetadata: true,
    })
    return {
      meta: fallback.meta as unknown as PackageMeta,
      pickedPackage: fallback.needsFullRefetch ? null : fallback.pickedPackage as unknown as PackageInRegistry | null,
    }
  }
  const result = await resolveMetadataDataInWorker({
    ...commonOpts,
    fullMetadata: true,
    jsonText: fullFetchResult.jsonText,
    etag: fullFetchResult.etag,
  })
  return {
    meta: result.meta as unknown as PackageMeta,
    pickedPackage: result.needsFullRefetch ? null : result.pickedPackage as unknown as PackageInRegistry | null,
  }
}
