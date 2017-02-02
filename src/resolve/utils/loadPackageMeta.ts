import url = require('url')
import registryUrl = require('registry-url')
import loadJsonFile = require('load-json-file')
import writeJsonFile = require('write-json-file')
import path = require('path')
import {Got} from '../../network/got'
import {PackageSpec} from '..'
import {Package} from '../../types'
import createPkgId from './createNpmPkgId'
import logger from 'pnpm-logger'

export type PackageMeta = {
  'dist-tag': { [name: string]: string },
  versions: {
    [name: string]: PackageInRegistry
  }
}

export type PackageInRegistry = Package & {
  dist: {
    shasum: string,
    tarball: string
  }
}

export default async function (
  spec: PackageSpec,
  storePath: string,
  got: Got,
  metaCache: Map<string, PackageMeta>,
): Promise<PackageMeta> {
  if (metaCache.has(spec.name)) {
    return <PackageMeta>metaCache.get(spec.name)
  }
  const meta = await loadPkgMetaNonCached(spec, storePath, got)
  metaCache.set(spec.name, meta)
  return meta
}

async function loadPkgMetaNonCached (
  spec: PackageSpec,
  storePath: string,
  got: Got
): Promise<PackageMeta> {
  const registry = (<string>url.parse(registryUrl(spec.scope)).host).replace(':', '+')
  const pkgStore = path.join(storePath, registry, spec.name)

  if (spec.type === 'version') {
    const meta = await loadMeta(pkgStore)
    // use the cached meta only if it has the required package version
    // otherwise it is probably out of date
    if (meta && meta.versions && meta.versions[spec.spec]) {
      return meta
    }
  }

  try {
    const meta = await fromRegistry(got, spec)
    saveMeta(pkgStore, meta)
    return meta
  } catch (err) {
    const meta = await loadMeta(storePath)
    if (!meta) throw err
    logger.error(err)
    logger.info(`Using cached meta from ${storePath}`)
    return meta
  }
}

async function fromRegistry (got: Got, spec: PackageSpec) {
  const uri = toUri(spec)
  const meta = <PackageMeta>await got.getJSON(uri)
  return meta
}

const META_FILENAME = 'meta.json'

async function loadMeta (pkgStore: string): Promise<PackageMeta | null> {
  try {
    return await loadJsonFile(path.join(pkgStore, META_FILENAME))
  } catch (err) {
    return null
  }
}

function saveMeta (pkgStore: string, meta: PackageMeta): Promise<PackageMeta> {
  return writeJsonFile(path.join(pkgStore, META_FILENAME), meta)
}

/**
 * Converts package data (from `npa()`) to a URI
 *
 * @example
 *     toUri({ name: 'rimraf', rawSpec: '2' })
 *     // => 'https://registry.npmjs.org/rimraf/2'
 */
function toUri (spec: PackageSpec) {
  let name: string

  if (spec.name.substr(0, 1) === '@') {
    name = '@' + encodeURIComponent(spec.name.substr(1))
  } else {
    name = encodeURIComponent(spec.name)
  }

  return url.resolve(registryUrl(spec.scope), name)
}
