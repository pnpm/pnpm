import url = require('url')
import loadJsonFile = require('load-json-file')
import writeJsonFile = require('write-json-file')
import path = require('path')
import {Got} from '../../network/got'
import {PackageSpec} from '..'
import {Package} from '../../types'
import createPkgId from './createNpmPkgId'
import getRegistryName = require('encode-registry')
import logger from 'pnpm-logger'
import pLimit = require('p-limit')
import {PnpmError} from '../../errorTypes'

export type PackageMeta = {
  'dist-tag': { [name: string]: string },
  versions: {
    [name: string]: PackageInRegistry
  }
}

export type PackageInRegistry = Package & {
  dist: {
    integrity?: string,
    shasum: string,
    tarball: string
  }
}

// prevents simultainous operations on the meta.json
// otherwise it would cause EPERM exceptions
const metafileOperationLimits = {}

export default async function loadPkgMetaNonCached (
  spec: PackageSpec,
  opts: {
    storePath: string,
    got: Got,
    metaCache: Map<string, PackageMeta>,
    offline: boolean,
    registry: string,
    downloadPriority: number,
  }
): Promise<PackageMeta> {
  opts = opts || {}

  if (opts.metaCache.has(spec.name)) {
    return <PackageMeta>opts.metaCache.get(spec.name)
  }

  const registryName = getRegistryName(opts.registry)
  const pkgMirror = path.join(opts.storePath, registryName, spec.name)
  const limit = metafileOperationLimits[pkgMirror] = metafileOperationLimits[pkgMirror] || pLimit(1)

  if (opts.offline) {
    const meta = await limit(() => loadMeta(pkgMirror))

    if (meta) return meta

    throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${spec.rawSpec} in package mirror ${pkgMirror}`)
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

async function fromRegistry (got: Got, spec: PackageSpec, registry: string, downloadPriority: number) {
  const uri = toUri(spec, registry)
  const meta = <PackageMeta>await got.getJSON(uri, registry, downloadPriority)
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
 *     // => 'https://registry.npmjs.org/rimraf/2'
 */
function toUri (spec: PackageSpec, registry: string) {
  let name: string

  if (spec.name.substr(0, 1) === '@') {
    name = '@' + encodeURIComponent(spec.name.substr(1))
  } else {
    name = encodeURIComponent(spec.name)
  }

  return url.resolve(registry, name)
}
