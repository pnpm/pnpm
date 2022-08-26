import crypto from 'crypto'
import { promises as fs } from 'fs'
import path from 'path'
import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import gfs from '@pnpm/graceful-fs'
import { VersionSelectors } from '@pnpm/resolver-base'
import { PackageManifest } from '@pnpm/types'
import getRegistryName from 'encode-registry'
import loadJsonFile from 'load-json-file'
import pLimit from 'p-limit'
import pathTemp from 'path-temp'
import renameOverwrite from 'rename-overwrite'
import toRaw from './toRaw'
import pickPackageFromMeta from './pickPackageFromMeta'
import { RegistryPackageSpec } from './parsePref'

export interface PackageMeta {
  'dist-tags': Record<string, string>
  versions: Record<string, PackageInRegistry>
  cachedAt?: number
}

export interface PackageMetaCache {
  get: (key: string) => PackageMeta | undefined
  set: (key: string, meta: PackageMeta) => void
  has: (key: string) => boolean
}

export type PackageInRegistry = PackageManifest & {
  dist: {
    integrity?: string
    shasum: string
    tarball: string
  }
}

/**
 * prevents simultaneous operations on the meta.json
 * otherwise it would cause EPERM exceptions
 */
const metafileOperationLimits = {} as {
  [pkgMirror: string]: pLimit.Limit
}

export interface PickPackageOptions {
  authHeaderValue?: string
  preferredVersionSelectors: VersionSelectors | undefined
  registry: string
  dryRun: boolean
}

export default async (
  ctx: {
    fetch: (pkgName: string, registry: string, authHeaderValue?: string) => Promise<PackageMeta>
    metaDir: string
    metaCache: PackageMetaCache
    cacheDir: string
    offline?: boolean
    preferOffline?: boolean
  },
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
): Promise<{meta: PackageMeta, pickedPackage: PackageInRegistry | null}> => {
  opts = opts || {}

  validatePackageName(spec.name)

  const cachedMeta = ctx.metaCache.get(spec.name)
  if (cachedMeta != null) {
    return {
      meta: cachedMeta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelectors, cachedMeta),
    }
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(ctx.cacheDir, ctx.metaDir, registryName, `${encodePkgName(spec.name)}.json`)
  const limit = metafileOperationLimits[pkgMirror] = metafileOperationLimits[pkgMirror] || pLimit(1)

  let metaCachedInStore: PackageMeta | null | undefined
  if (ctx.offline === true || ctx.preferOffline) {
    metaCachedInStore = await limit(async () => loadMeta(pkgMirror))

    if (ctx.offline) {
      if (metaCachedInStore != null) return {
        meta: metaCachedInStore,
        pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelectors, metaCachedInStore),
      }

      throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
    }

    if (metaCachedInStore != null) {
      const pickedPackage = pickPackageFromMeta(spec, opts.preferredVersionSelectors, metaCachedInStore)
      if (pickedPackage) {
        return {
          meta: metaCachedInStore,
          pickedPackage,
        }
      }
    }
  }

  if (spec.type === 'version') {
    metaCachedInStore = metaCachedInStore ?? await limit(async () => loadMeta(pkgMirror))
    // use the cached meta only if it has the required package version
    // otherwise it is probably out of date
    if ((metaCachedInStore?.versions?.[spec.fetchSpec]) != null) {
      return {
        meta: metaCachedInStore,
        pickedPackage: metaCachedInStore.versions[spec.fetchSpec],
      }
    }
  }

  try {
    const meta = await ctx.fetch(spec.name, opts.registry, opts.authHeaderValue)
    meta.cachedAt = Date.now()
    // only save meta to cache, when it is fresh
    ctx.metaCache.set(spec.name, meta)
    if (!opts.dryRun) {
      // eslint-disable-next-line @typescript-eslint/no-floating-promises
      limit(async () => {
        try {
          await saveMeta(pkgMirror, meta)
        } catch (err: any) { // eslint-disable-line
          // We don't care if this file was not written to the cache
        }
      })
    }
    return {
      meta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelectors, meta),
    }
  } catch (err: any) { // eslint-disable-line
    err.spec = spec
    const meta = await loadMeta(pkgMirror) // TODO: add test for this usecase
    if (meta == null) throw err
    logger.error(err, err)
    logger.debug({ message: `Using cached meta from ${pkgMirror}` })
    return {
      meta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelectors, meta),
    }
  }
}

function encodePkgName (pkgName: string) {
  if (pkgName !== pkgName.toLowerCase()) {
    return `${pkgName}_${crypto.createHash('md5').update(pkgName).digest('hex')}`
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

async function saveMeta (pkgMirror: string, meta: PackageMeta): Promise<void> {
  const dir = path.dirname(pkgMirror)
  if (!createdDirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    createdDirs.add(dir)
  }
  const temp = pathTemp(dir)
  await gfs.writeFile(temp, JSON.stringify(meta))
  await renameOverwrite(temp, pkgMirror)
}

function validatePackageName (pkgName: string) {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}
