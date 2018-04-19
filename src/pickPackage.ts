import logger from '@pnpm/logger'
import {PackageManifest} from '@pnpm/types'
import getRegistryName = require('encode-registry')
import loadJsonFile = require('load-json-file')
import pLimit = require('p-limit')
import path = require('path')
import url = require('url')
import writeJsonFile = require('write-json-file')
import {RegistryPackageSpec} from './parsePref'
import pickPackageFromMeta from './pickPackageFromMeta'
import toRaw from './toRaw'

const DEFAULT_CACHE_TTL = 120 * 1000 // 2 minutes

class PnpmError extends Error {
  public code: string
  constructor (code: string, message: string) {
    super(message)
    this.code = code
  }
}

export interface PackageMeta {
  'dist-tag': { [name: string]: string },
  versions: {
    [name: string]: PackageInRegistry,
  }
  cachedAt?: number,
}

export type PackageInRegistry = PackageManifest & {
  dist: {
    integrity?: string,
    shasum: string,
    tarball: string,
  },
}

// prevents simultainous operations on the meta.json
// otherwise it would cause EPERM exceptions
const metafileOperationLimits = {}

export default async (
  ctx: {
    fetch: (url: string, opts: {auth?: object}) => Promise<{}>,
    fullMetadata: boolean,
    metaCache: Map<string, object>,
    storePath: string,
    offline: boolean,
    preferOffline: boolean,
    cacheTtl?: number,
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
  ctx.cacheTtl = ctx.cacheTtl || DEFAULT_CACHE_TTL

  if (ctx.metaCache.has(spec.name)) {
    const meta = ctx.metaCache.get(spec.name) as PackageMeta
    if (meta.cachedAt && Date.now() - meta.cachedAt < ctx.cacheTtl) {
      return {
        meta,
        pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, meta),
      }
    }
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(ctx.storePath, registryName, spec.name)
  const limit = metafileOperationLimits[pkgMirror] = metafileOperationLimits[pkgMirror] || pLimit(1)

  let metaCachedInStore: PackageMeta | undefined
  if (ctx.offline || ctx.preferOffline) {
    metaCachedInStore = await limit(() => loadMeta(ctx.fullMetadata, pkgMirror))

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
    metaCachedInStore = metaCachedInStore || await limit(() => loadMeta(ctx.fullMetadata, pkgMirror))
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
      limit(() => saveMeta(pkgMirror, meta))
    }
    return {
      meta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, meta),
    }
  } catch (err) {
    const meta = await loadMeta(ctx.fullMetadata, pkgMirror) // TODO: add test for this usecase
    if (!meta) throw err
    logger.error(err)
    logger.info(`Using cached meta from ${pkgMirror}`)
    return {
      meta,
      pickedPackage: pickPackageFromMeta(spec, opts.preferredVersionSelector, meta),
    }
  }
}

async function fromRegistry (
  fetch: (url: string, opts: {auth?: object}) => Promise<{}>,
  pkgName: string,
  registry: string,
  auth: object,
) {
  const uri = toUri(pkgName, registry)
  const res = await fetch(uri, {auth}) as {
    status: number,
    statusText: string,
    json: () => Promise<PackageMeta>,
  }
  if (res.status > 400) {
    const err = new Error(`${res.status} ${res.statusText}: ${pkgName}`)
    // tslint:disable
    err['code'] = `E${res.status}`
    err['uri'] = uri
    err['response'] = res
    err['package'] = pkgName
    // tslint:enable
    throw err
  }
  return await res.json()
}

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_FILENAME = 'index.json'
const FULL_META_FILENAME = 'index-full.json'

async function loadMeta (fullMetadata: boolean, pkgMirror: string): Promise<PackageMeta | null> {
  try {
    return await loadJsonFile(path.join(pkgMirror, fullMetadata ? FULL_META_FILENAME : META_FILENAME))
  } catch (err) {
    return null
  }
}

function saveMeta (pkgMirror: string, meta: PackageMeta): Promise<void> {
  return writeJsonFile(path.join(pkgMirror, META_FILENAME), meta)
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
