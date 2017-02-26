import url = require('url')
import registryUrl = require('registry-url')
import loadJsonFile = require('load-json-file')
import writeJsonFile = require('write-json-file')
import path = require('path')
import {Got} from '../../network/got'
import {PackageSpec} from '..'
import {Package} from '../../types'
import createPkgId from './createNpmPkgId'
import getRegistryFolderName from './getRegistryFolderName'
import logger from 'pnpm-logger'
import pLimit = require('p-limit')

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

// prevents simultainous operations on the meta.json
// otherwise it would cause EPERM exceptions
const metafileOperationLimits = {}

export default async function loadPkgMetaNonCached (
  spec: PackageSpec,
  localRegistry: string,
  got: Got,
  metaCache: Map<string, PackageMeta>,
): Promise<PackageMeta> {
  if (metaCache.has(spec.name)) {
    return <PackageMeta>metaCache.get(spec.name)
  }

  const registry = getRegistryFolderName(registryUrl(spec.scope))
  const pkgMirror = path.join(localRegistry, registry, spec.name)
  const limit = metafileOperationLimits[pkgMirror] = metafileOperationLimits[pkgMirror] || pLimit(1)

  if (spec.type === 'version') {
    const meta = await limit(() => loadMeta(pkgMirror))
    // use the cached meta only if it has the required package version
    // otherwise it is probably out of date
    if (meta && meta.versions && meta.versions[spec.spec]) {
      return meta
    }
  }

  try {
    const meta = await fromRegistry(got, spec)
    // only save meta to cache, when it is fresh
    metaCache.set(spec.name, meta)
    limit(() => saveMeta(pkgMirror, meta))
    return meta
  } catch (err) {
    const meta = await loadMeta(localRegistry)
    if (!meta) throw err
    logger.error(err)
    logger.info(`Using cached meta from ${localRegistry}`)
    return meta
  }
}

async function fromRegistry (got: Got, spec: PackageSpec) {
  const uri = toUri(spec)
  const meta = <PackageMeta>await got.getJSON(uri)
  return meta
}

// Don't let the name confuse you, this file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_FILENAME = 'package.json'

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
function toUri (spec: PackageSpec) {
  let name: string

  if (spec.name.substr(0, 1) === '@') {
    name = '@' + encodeURIComponent(spec.name.substr(1))
  } else {
    name = encodeURIComponent(spec.name)
  }

  return url.resolve(registryUrl(spec.scope), name)
}
