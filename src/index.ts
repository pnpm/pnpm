import {PackageManifest} from '@pnpm/types'
import path = require('path')
import semver = require('semver')
import ssri = require('ssri')
import url = require('url')
import createPkgId from './createNpmPkgId'
import loadPkgMeta, {PackageMeta} from './loadPackageMeta'
import parsePref from './parsePref'
import toRaw from './toRaw'

export {
  PackageManifest,
  PackageMeta,
}

export default async function resolveNpm (
  wantedDependency: {
    alias?: string,
    pref: string,
  },
  opts: {
    storePath: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    offline: boolean,
    getJson<T> (url: string, registry: string): Promise<T>,
  },
) {
  const spec = parsePref(wantedDependency.pref, wantedDependency.alias)
  if (!spec) return null
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  try {
    const meta = await loadPkgMeta(spec, {
      getJson: opts.getJson,
      metaCache: opts.metaCache,
      offline: opts.offline,
      registry: opts.registry,
      storePath: opts.storePath,
    })
    const correctPkg = spec.type === 'tag'
      ? pickVersionByTag(meta, spec.fetchSpec)
      : pickVersionByVersionRange(meta, spec.fetchSpec)
    if (!correctPkg) {
      const versions = Object.keys(meta.versions)
      const message = versions.length
        ? 'Versions in registry:\n' + versions.join(', ') + '\n'
        : 'No valid version found.'
      const err = new Error('No compatible version found: ' +
        toRaw(spec) + '\n' + message)
      throw err
    }
    const id = createPkgId(correctPkg.dist.tarball, correctPkg.name, correctPkg.version)

    const resolution = {
      integrity: getIntegrity(correctPkg.dist),
      registry: opts.registry,
      tarball: correctPkg.dist.tarball,
    }
    return {
      id,
      latest: meta['dist-tags'].latest,
      package: correctPkg,
      resolution,
    }
  } catch (err) {
    if (err.statusCode === 404) {
      throw new Error(`Module '${toRaw(spec)}' not found`)
    }
    throw err
  }
}

function getIntegrity (dist: {
  integrity?: string,
  shasum: string,
  tarball: string,
}) {
  if (dist.integrity) {
    return dist.integrity
  }
  return ssri.fromHex(dist.shasum, 'sha1').toString()
}

function pickVersionByTag (meta: PackageMeta, tag: string) {
  const tagVersion = meta['dist-tags'][tag]
  if (meta.versions[tagVersion]) {
    return meta.versions[tagVersion]
  }
  return null
}

function pickVersionByVersionRange (meta: PackageMeta, versionRange: string) {
  const latest = meta['dist-tags'].latest

  // Not using semver.satisfies in case of * because it does not select beta versions.
  // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
  if (versionRange === '*' || semver.satisfies(latest, versionRange, true)) {
    return meta.versions[latest]
  }
  const versions = Object.keys(meta.versions)
  const maxVersion = semver.maxSatisfying(versions, versionRange, true)
  if (maxVersion) {
    return meta.versions[maxVersion]
  }
  return null
}
