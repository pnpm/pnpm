import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { PackageManifest } from '@pnpm/types'
import getRegistryName = require('encode-registry')
import loadJsonFile = require('load-json-file')
import pLimit, { Limit } from 'p-limit'
import path = require('path')
import url = require('url')
import writeJsonFile = require('write-json-file')
import { RegistryPackageSpec } from './parsePref'
import pickPackageFromMeta from './pickPackageFromMeta'
import toRaw from './toRaw'

export interface PackageMeta {
  'dist-tag': { [name: string]: string },
  versions: {
    [name: string]: PackageInRegistry,
  }
  cachedAt?: number,
}

export interface PackageMetaCache {
  get (key: string): PackageMeta | undefined
  set (key: string, meta: PackageMeta): void
  has (key: string): boolean
}

export type PackageInRegistry = PackageManifest & {
  dist: {
    integrity?: string,
    shasum: string,
    tarball: string,
  },
}

/**
 * prevents simultaneous operations on the meta.json
 * otherwise it would cause EPERM exceptions
 */
const metafileOperationLimits = {} as {
  [pkgMirror: string]: Limit
}

export default async (
  ctx: {
    fetch: (url: string, opts: {auth?: object}) => Promise<{}>,
    metaFileName: string,
    metaCache: PackageMetaCache,
    storePath: string,
    offline?: boolean,
    preferOffline?: boolean,
  },
  spec: RegistryPackageSpec,
  opts: {
    auth: object,
    preferredVersionSelector: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    } | undefined,
    registry: string,
    dryRun: boolean,
  },
): Promise<{meta: PackageMeta, pickedPackage: PackageInRegistry | null}> => {
  opts = opts || {}

  const cachedMeta = ctx.metaCache.get(spec.name)
  if (cachedMeta) {
    return {
      meta: cachedMeta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, cachedMeta),
    }
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(ctx.storePath, registryName, spec.name)
  const limit = metafileOperationLimits[pkgMirror] = metafileOperationLimits[pkgMirror] || pLimit(1)

  let metaCachedInStore: PackageMeta | null | undefined
  if (ctx.offline || ctx.preferOffline) {
    metaCachedInStore = await limit(() => loadMeta(pkgMirror, ctx.metaFileName))

    if (ctx.offline) {
      if (metaCachedInStore) return {
        meta: metaCachedInStore,
        pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, metaCachedInStore),
      }

      throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
    }

    if (metaCachedInStore) {
      const pickedPackage = pickPackageFromMeta(spec, opts.preferredVersionSelector, metaCachedInStore)
      if (pickedPackage) {
        return {
          meta: metaCachedInStore,
          pickedPackage,
        }
      }
    }
  }

  if (spec.type === 'version') {
    metaCachedInStore = metaCachedInStore || await limit(() => loadMeta(pkgMirror, ctx.metaFileName))
    // use the cached meta only if it has the required package version
    // otherwise it is probably out of date
    if (metaCachedInStore && metaCachedInStore.versions && metaCachedInStore.versions[spec.fetchSpec]) {
      return {
        meta: metaCachedInStore,
        pickedPackage: metaCachedInStore.versions[spec.fetchSpec],
      }
    }
  }

  try {
    const meta = await fromRegistry(ctx.fetch, spec.name, opts.registry, opts.auth)
    meta.cachedAt = Date.now()
    // only save meta to cache, when it is fresh
    ctx.metaCache.set(spec.name, meta)
    if (!opts.dryRun) {
      // tslint:disable-next-line:no-floating-promises
      limit(() => saveMeta(pkgMirror, meta, ctx.metaFileName))
    }
    return {
      meta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, meta),
    }
  } catch (err) {
    const meta = await loadMeta(pkgMirror, ctx.metaFileName) // TODO: add test for this usecase
    if (!meta) throw err
    logger.error(err)
    logger.debug({ message: `Using cached meta from ${pkgMirror}` })
    return {
      meta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, meta),
    }
  }
}

type RegistryResponse = {
  status: number,
  statusText: string,
  json: () => Promise<PackageMeta>,
}

class RegistryResponseError extends PnpmError {
  public readonly package: string
  public readonly response: RegistryResponse
  public readonly uri: string

  constructor (opts: {
    package: string,
    response: RegistryResponse,
    uri: string,
  }) {
    super(
      `REGISTRY_META_RESPONSE_${opts.response.status}`,
      `${opts.response.status} ${opts.response.statusText}: ${opts.package} (via ${opts.uri})`)
    this.package = opts.package
    this.response = opts.response
    this.uri = opts.uri
  }
}

async function fromRegistry (
  fetch: (url: string, opts: {auth?: object}) => Promise<{}>,
  pkgName: string,
  registry: string,
  auth?: object,
) {
  const uri = toUri(pkgName, registry)
  const response = await fetch(uri, { auth }) as RegistryResponse
  if (response.status > 400) {
    throw new RegistryResponseError({
      package: pkgName,
      response,
      uri,
    })
  }
  return response.json()
}

async function loadMeta (pkgMirror: string, metaFileName: string): Promise<PackageMeta | null> {
  try {
    return await loadJsonFile<PackageMeta>(path.join(pkgMirror, metaFileName))
  } catch (err) {
    return null
  }
}

function saveMeta (pkgMirror: string, meta: PackageMeta, metaFileName: string): Promise<void> {
  return writeJsonFile(path.join(pkgMirror, metaFileName), meta)
}

function toUri (pkgName: string, registry: string) {
  let encodedName: string

  if (pkgName[0] === '@') {
    encodedName = `@${encodeURIComponent(pkgName.substr(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }

  return url.resolve(registry, encodedName)
}
