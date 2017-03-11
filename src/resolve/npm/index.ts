import url = require('url')
import path = require('path')
import semver = require('semver')
import {PackageSpec, ResolveOptions, TarballResolution, ResolveResult} from '..'
import logStatus from '../../logging/logInstallStatus'
import loadPkgMeta, {PackageMeta} from './loadPackageMeta'
import createPkgId from './createNpmPkgId'
import getHost from './getHost'

export {PackageMeta}

/**
 * Resolves a package in the NPM registry. Done as part of `install()`.
 *
 * @example
 *     var npa = require('npm-package-arg')
 *     resolve(npa('rimraf@2'))
 *       .then((res) => {
 *         res.id == 'rimraf@2.5.1'
 *         res.dist == {
 *           shasum: '0a1b2c...'
 *           tarball: 'http://...'
 *         }
 *       })
 */
export default async function resolveNpm (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  try {
    if (opts.loggedPkg) logStatus({ status: 'resolving', pkg: opts.loggedPkg })
    const meta = await loadPkgMeta(spec, {
      localRegistry: opts.localRegistry,
      got: opts.got,
      metaCache: opts.metaCache,
      offline: opts.offline,
    })
    const correctPkg = pickVersion(meta, spec)
    if (!correctPkg) {
      const versions = Object.keys(meta.versions)
      const message = versions.length
        ? 'Versions in registry:\n' + versions.join(', ') + '\n'
        : 'No valid version found.'
      const err = new Error('No compatible version found: ' +
        spec.raw + '\n' + message)
      throw err
    }
    const registryHost = getHost(correctPkg.dist.tarball)
    const id = createPkgId(registryHost, correctPkg.name, correctPkg.version)

    const resolution: TarballResolution = {
      shasum: correctPkg.dist.shasum,
      tarball: correctPkg.dist.tarball,
    }
    return {id, resolution, package: correctPkg}
  } catch (err) {
    if (err['statusCode'] === 404) {
      throw new Error("Module '" + spec.raw + "' not found")
    }
    throw err
  }
}

function pickVersion (meta: PackageMeta, dep: PackageSpec) {
  if (dep.type === 'tag') {
    return pickVersionByTag(meta, dep.spec)
  }
  return pickVersionByVersionRange(meta, dep.spec)
}

function pickVersionByTag(meta: PackageMeta, tag: string) {
  const tagVersion = meta['dist-tags'][tag]
  if (meta.versions[tagVersion]) {
    return meta.versions[tagVersion]
  }
  return null
}

function pickVersionByVersionRange(meta: PackageMeta, versionRange: string) {
  const latest = meta['dist-tags']['latest']
  if (semver.satisfies(latest, versionRange, true)) {
    return meta.versions[latest]
  }
  const versions = Object.keys(meta.versions)
  const maxVersion = semver.maxSatisfying(versions, versionRange, true)
  if (maxVersion) {
    return meta.versions[maxVersion]
  }
  return null
}
