import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import getRegistryName = require('encode-registry')
import loadJsonFile = require('load-json-file')
import pLimit = require('p-limit')
import path = require('path')
import url = require('url')
import writeJsonFile = require('write-json-file')
import {PnpmError} from '../../errorTypes'
import {Got} from '../../network/got'
import createPkgId from './createNpmPkgId'
import {RegistryPackageSpec} from './parsePref'
import toRaw from './toRaw'

export interface PackageMeta {
  'dist-tag': { [name: string]: string },
  versions: {
    [name: string]: PackageInRegistry,
  }
}

export type PackageInRegistry = PackageJson & {
  dist: {
    integrity?: string,
    shasum: string,
    tarball: string,
  },
}

// prevents simultainous operations on the meta.json
// otherwise it would cause EPERM exceptions
const metafileOperationLimits = {}

export default async function loadPkgMetaNonCached (
  spec: RegistryPackageSpec,
  opts: {
    storePath: string,
    got: Got,
    metaCache: Map<string, PackageMeta>,
    offline: boolean,
    registry: string,
    downloadPriority: number,
  },
): Promise<PackageMeta> {
  opts = opts || {}

  if (opts.metaCache.has(spec.name)) {
    return opts.metaCache.get(spec.name) as PackageMeta
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(opts.storePath, registryName, spec.name)
  const limit = metafileOperationLimits[pkgMirror] = metafileOperationLimits[pkgMirror] || pLimit(1)

  if (opts.offline) {
    const meta = await limit(() => loadMeta(pkgMirror))

    if (meta) return meta

    throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${toRaw(spec)} in package mirror ${pkgMirror}`)
  }

  if (spec.type === 'version') {
    const meta = await limit(() => loadMeta(pkgMirror))
    // use the cached meta only if it has the required package version
    // otherwise it is probably out of date
    if (meta && meta.versions && meta.versions[spec.fetchSpec]) {
      return meta
    }
  }

  try {
    const meta = await fromRegistry(opts.got, spec, opts.registry, opts.downloadPriority)
    // only save meta to cache, when it is fresh
    opts.metaCache.set(spec.name, meta)
    limit(() => saveMeta(pkgMirror, meta))
    return meta
  } catch (err) {
    const meta = await loadMeta(opts.storePath)
    if (!meta) throw err
    logger.error(err)
    logger.info(`Using cached meta from ${opts.storePath}`)
    return meta
  }
}

async function fromRegistry (got: Got, spec: RegistryPackageSpec, registry: string, downloadPriority: number) {
  const uri = toUri(spec, registry)
  const meta = await got.getJSON(uri, registry, downloadPriority) as PackageMeta
  return meta
}

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_FILENAME = 'index.json'

async function loadMeta (pkgMirror: string): Promise<PackageMeta | null> {
  try {
    return await loadJsonFile(path.join(pkgMirror, META_FILENAME))
  } catch (err) {
    return null
  }
}

function saveMeta (pkgMirror: string, meta: PackageMeta): Promise<PackageMeta> {
  return writeJsonFile(path.join(pkgMirror, META_FILENAME), meta)
}

/**
 * Converts package data (from `npa()`) to a URI
 *
 * @example
 *     toUri({ name: 'rimraf', rawSpec: '2' })
 *     // => 'https://registry.npmjs.org/rimraf'
 *
 * Although it is possible to download the needed package.json with one request
 * by passing the spec like this: 'https://registry.npmjs.org/rimraf/2'
 * This increases the number of HTTP requests during installation and slows down
 * pnpm up to twice!
 */
function toUri (spec: RegistryPackageSpec, registry: string) {
  let name: string

  if (spec.name.substr(0, 1) === '@') {
    name = '@' + encodeURIComponent(spec.name.substr(1))
  } else {
    name = encodeURIComponent(spec.name)
  }

  return url.resolve(registry, name)
}
