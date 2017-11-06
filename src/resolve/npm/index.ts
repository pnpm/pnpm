import path = require('path')
import {progressLogger} from 'pnpm-logger'
import semver = require('semver')
import ssri = require('ssri')
import url = require('url')
import {PackageSpec, ResolveOptions, ResolveResult, TarballResolution} from '..'
import createPkgId from './createNpmPkgId'
import loadPkgMeta, {PackageMeta} from './loadPackageMeta'

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
    if (opts.loggedPkg) {
      progressLogger.debug({ status: 'resolving', pkg: opts.loggedPkg })
    }
    const meta = await loadPkgMeta(spec, {
      downloadPriority: opts.downloadPriority,
      got: opts.got,
      metaCache: opts.metaCache,
      offline: opts.offline,
      registry: opts.registry,
      storePath: opts.storePath,
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
    const id = createPkgId(correctPkg.dist.tarball, correctPkg.name, correctPkg.version)

    const resolution: TarballResolution = {
      integrity: getIntegrity(correctPkg.dist),
      registry: opts.registry,
      tarball: correctPkg.dist.tarball,
    }
    return {id, resolution, package: correctPkg}
  } catch (err) {
    if (err.statusCode === 404) {
      throw new Error("Module '" + spec.raw + "' not found")
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

function pickVersion (meta: PackageMeta, dep: PackageSpec) {
  if (dep.type === 'tag') {
    return pickVersionByTag(meta, dep.fetchSpec)
  }
  return pickVersionByVersionRange(meta, dep.fetchSpec)
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
