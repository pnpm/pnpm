import { promises as fs } from 'fs'
import path from 'path'
import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import gfs from '@pnpm/graceful-fs'
import { type PackageMeta, type PackageInRegistry } from '@pnpm/registry.types'
import getRegistryName from 'encode-registry'
import { loadJsonFile } from 'load-json-file'
import pLimit, { type LimitFunction } from 'p-limit'
import { fastPathTemp as pathTemp } from 'path-temp'
import { pick } from 'ramda'
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
  updateToLatest?: boolean
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
    fetch: (pkgName: string, registry: string, authHeaderValue?: string) => Promise<PackageMeta>
    metaDir: string
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

  const cachedMeta = ctx.metaCache.get(spec.name)
  if (cachedMeta != null) {
    return {
      meta: cachedMeta,
      pickedPackage: _pickPackageFromMeta(cachedMeta),
    }
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(ctx.cacheDir, ctx.metaDir, registryName, `${encodePkgName(spec.name)}.json`)

  return runLimited(pkgMirror, async (limit) => {
    let metaCachedInStore: PackageMeta | null | undefined
    if (ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion) {
      metaCachedInStore = await limit(async () => loadMeta(pkgMirror))

      // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
      if (ctx.offline || ctx.preferOffline) {
        if (metaCachedInStore != null) return {
          meta: metaCachedInStore,
          pickedPackage: _pickPackageFromMeta(metaCachedInStore),
        }

        if (ctx.offline) throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
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
        } catch (err) {
          if (ctx.strictPublishedByCheck) {
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
        } catch (err) {
          if (ctx.strictPublishedByCheck) {
            throw err
          }
        }
      }
    }

    try {
      let meta = await ctx.fetch(spec.name, opts.registry, opts.authHeaderValue)
      if (ctx.filterMetadata) {
        meta = clearMeta(meta)
      }
      meta.cachedAt = Date.now()
      // only save meta to cache, when it is fresh
      ctx.metaCache.set(spec.name, meta)
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
