import path from 'node:path'
import crypto from 'node:crypto'
import { promises as fs } from 'node:fs'

import semver from 'semver'
import pLimit from 'p-limit'
import pick from 'ramda/src/pick'
import loadJsonFile from 'load-json-file'
import getRegistryName from 'encode-registry'
import renameOverwrite from 'rename-overwrite'
import { fastPathTemp as pathTemp } from 'path-temp'

import gfs from '@pnpm/graceful-fs'
import { logger } from '@pnpm/logger'
import { PnpmError } from '@pnpm/error'
import type { RefCountedLimiter, VersionSelectors, PickPackageOptions, PackageMeta, PackageInRegistry, PackageMetaCache, RegistryPackageSpec } from '@pnpm/types'

import {
  pickPackageFromMeta,
  pickVersionByVersionRange,
  pickLowestVersionByVersionRange,
} from './pickPackageFromMeta.js'
import { toRaw } from './toRaw.js'

/**
 * prevents simultaneous operations on the meta.json
 * otherwise it would cause EPERM exceptions
 */
const metafileOperationLimits: Record<string, RefCountedLimiter | undefined> = {}

/**
 * To prevent metafileOperationLimits from holding onto objects in memory on
 * the order of the number of packages, refcount the limiters and drop them
 * once they are no longer needed. Callers of this function should ensure
 * that the limiter is no longer referenced once fn's Promise has resolved.
 */
async function runLimited<T>(
  pkgMirror: string,
  fn: (limit: pLimit.Limit) => Promise<T>
): Promise<T> {
  let entry: RefCountedLimiter | undefined

  try {
    entry = metafileOperationLimits[pkgMirror] ??= {
      count: 0,
      limit: pLimit(1),
    }

    entry.count++
    return await fn(entry.limit)
  } finally {
    if (entry) {
      entry.count--

      if (entry.count === 0) {
        metafileOperationLimits[pkgMirror] = undefined
      }
    }
  }
}

function pickPackageFromMetaUsingTime(
  spec: RegistryPackageSpec,
  preferredVersionSelectors: VersionSelectors | undefined,
  meta: PackageMeta,
  publishedBy?: Date | undefined
): PackageInRegistry | null | undefined {
  const pickedPackage = pickPackageFromMeta(
    pickVersionByVersionRange,
    spec,
    preferredVersionSelectors,
    meta,
    publishedBy
  )
  if (pickedPackage) return pickedPackage
  return pickPackageFromMeta(
    pickLowestVersionByVersionRange,
    spec,
    preferredVersionSelectors,
    meta,
    publishedBy
  )
}

export async function pickPackage(
  ctx: {
    fetch: (
      pkgName: string,
      registry: string,
      authHeaderValue?: string | undefined
    ) => Promise<PackageMeta>
    metaDir: string
    metaCache: PackageMetaCache
    cacheDir: string
    offline?: boolean | undefined
    preferOffline?: boolean | undefined
    filterMetadata?: boolean | undefined
  },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): Promise<{ meta: PackageMeta; pickedPackage: PackageInRegistry | null | undefined }> {
  opts = opts ?? {}

  let _pickPackageFromMeta = opts.publishedBy
    ? pickPackageFromMetaUsingTime
    : pickPackageFromMeta.bind(
      null,
      opts.pickLowestVersion
        ? pickLowestVersionByVersionRange
        : pickVersionByVersionRange
    )

  if (opts.updateToLatest) {
    const _pickPackageBase = _pickPackageFromMeta

    _pickPackageFromMeta = (spec, ...rest) => {
      const latestStableSpec: RegistryPackageSpec = {
        ...spec,
        type: 'tag',
        fetchSpec: 'latest',
      }

      const latestStable = _pickPackageBase(latestStableSpec, ...rest)

      const current = _pickPackageBase(spec, ...rest)

      if (!latestStable) {
        return current
      }

      if (!current) {
        return latestStable
      }

      if (semver.lt(latestStable.version ?? '', current.version ?? '')) {
        return current
      }

      return latestStable
    }
  }

  validatePackageName(spec.name)

  const cachedMeta = ctx.metaCache.get(spec.name)
  if (cachedMeta != null) {
    return {
      meta: cachedMeta,
      pickedPackage: _pickPackageFromMeta(
        spec,
        opts.preferredVersionSelectors,
        cachedMeta,
        opts.publishedBy
      ),
    }
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(
    ctx.cacheDir,
    ctx.metaDir,
    registryName,
    `${encodePkgName(spec.name)}.json`
  )

  return runLimited(pkgMirror, async (limit) => {
    let metaCachedInStore: PackageMeta | null | undefined
    if (
      ctx.offline === true ||
      ctx.preferOffline === true ||
      opts.pickLowestVersion
    ) {
      metaCachedInStore = await limit(async () => loadMeta(pkgMirror))

      if (ctx.offline) {
        if (metaCachedInStore != null)
          return {
            meta: metaCachedInStore,
            pickedPackage: _pickPackageFromMeta(
              spec,
              opts.preferredVersionSelectors,
              metaCachedInStore,
              opts.publishedBy
            ),
          }

        throw new PnpmError(
          'NO_OFFLINE_META',
          `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`
        )
      }

      if (metaCachedInStore != null) {
        const pickedPackage = _pickPackageFromMeta(
          spec,
          opts.preferredVersionSelectors,
          metaCachedInStore,
          opts.publishedBy
        )
        if (pickedPackage) {
          return {
            meta: metaCachedInStore,
            pickedPackage,
          }
        }
      }
    }

    if (!opts.updateToLatest && spec.type === 'version') {
      metaCachedInStore =
        metaCachedInStore ?? (await limit(async () => loadMeta(pkgMirror)))
      // use the cached meta only if it has the required package version
      // otherwise it is probably out of date
      if (metaCachedInStore?.versions?.[spec.fetchSpec] != null) {
        return {
          meta: metaCachedInStore,
          pickedPackage: metaCachedInStore.versions[spec.fetchSpec],
        }
      }
    }
    if (opts.publishedBy) {
      metaCachedInStore =
        metaCachedInStore ?? (await limit(async () => loadMeta(pkgMirror)))
      if (
        metaCachedInStore?.cachedAt &&
        new Date(metaCachedInStore.cachedAt) >= opts.publishedBy
      ) {
        const pickedPackage = _pickPackageFromMeta(
          spec,
          opts.preferredVersionSelectors,
          metaCachedInStore,
          opts.publishedBy
        )
        if (pickedPackage) {
          return {
            meta: metaCachedInStore,
            pickedPackage,
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
        runLimited(pkgMirror, (limit) =>
          limit(async () => {
            try {
              await saveMeta(pkgMirror, meta)
          } catch (err: any) { // eslint-disable-line
              // We don't care if this file was not written to the cache
            }
          })
        )
      }
      return {
        meta,
        pickedPackage: _pickPackageFromMeta(
          spec,
          opts.preferredVersionSelectors,
          meta,
          opts.publishedBy
        ),
      }
    } catch (err: any) { // eslint-disable-line
      err.spec = spec
      const meta = await loadMeta(pkgMirror) // TODO: add test for this usecase
      if (meta == null) throw err
      logger.error(err, err)
      logger.debug({ message: `Using cached meta from ${pkgMirror}` })
      return {
        meta,
        pickedPackage: _pickPackageFromMeta(
          spec,
          opts.preferredVersionSelectors,
          meta,
          opts.publishedBy
        ),
      }
    }
  })
}

function clearMeta(pkg: PackageMeta): PackageMeta {
  const versions: PackageMeta['versions'] = {}
  for (const [version, info] of Object.entries(pkg.versions)) {
    // The list taken from https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object
    versions[version] = pick(
      [
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
        'deprecated',
        'bundleDependencies',
        'bundledDependencies',
        'hasInstallScript',
      ],
      info
    )
  }

  return {
    name: pkg.name,
    'dist-tags': pkg['dist-tags'],
    versions,
    time: pkg.time,
    cachedAt: pkg.cachedAt,
  }
}

function encodePkgName(pkgName: string) {
  if (pkgName !== pkgName.toLowerCase()) {
    return `${pkgName}_${crypto.createHash('md5').update(pkgName).digest('hex')}`
  }
  return pkgName
}

async function loadMeta(pkgMirror: string): Promise<PackageMeta | null> {
  try {
    return await loadJsonFile<PackageMeta>(pkgMirror)
  } catch (err: any) { // eslint-disable-line
    return null
  }
}

const createdDirs = new Set<string>()

async function saveMeta(pkgMirror: string, meta: PackageMeta): Promise<void> {
  const dir = path.dirname(pkgMirror)
  if (!createdDirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    createdDirs.add(dir)
  }
  const temp = pathTemp(pkgMirror)
  await gfs.writeFile(temp, JSON.stringify(meta))
  await renameOverwrite(temp, pkgMirror)
}

function validatePackageName(pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError(
      'INVALID_PACKAGE_NAME',
      `Package name ${pkgName} is invalid, it should have a @scope`
    )
  }
}
