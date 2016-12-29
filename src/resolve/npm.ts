import url = require('url')
const enc = encodeURIComponent
import createPkgId from './createPkgId'
import registryUrl = require('registry-url')
import semver = require('semver')
import {PackageSpec, ResolveOptions, ResolveResult} from '.'
import {Package} from '../types'
import {createRemoteTarballFetcher} from './fetch'

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
    const url = toUri(spec)
    if (opts.log) opts.log('resolving')
    const parsedBody = <PackageDocument>(await opts.got.getJSON(url))
    const correctPkg = pickVersionFromRegistryDocument(parsedBody, spec, opts.tag)
    if (!correctPkg) {
      const versions = Object.keys(parsedBody.versions)
      const message = versions.length
        ? 'Versions in registry:\n' + versions.join(', ') + '\n'
        : 'No valid version found.'
      const err = new Error('No compatible version found: ' +
        spec.raw + '\n' + message)
      throw err
    }
    return {
      id: createPkgId(correctPkg),
      pkg: correctPkg,
      fetch: createRemoteTarballFetcher({
        shasum: correctPkg.dist.shasum,
        tarball: correctPkg.dist.tarball
      }, opts)
    }
  } catch (err) {
    if (err['statusCode'] === 404) {
      throw new Error("Module '" + spec.raw + "' not found")
    }
    throw err
  }
}

type StringDict = {
  [name: string]: string
}

type PackageInRegistry = Package & {
  dist: {
    shasum: string,
    tarball: string
  }
}

type PackageDocument = {
  'dist-tag': StringDict,
  versions: {
    [name: string]: PackageInRegistry
  }
}

function pickVersionFromRegistryDocument (pkg: PackageDocument, dep: PackageSpec, latestTag: string) {
  if (dep.type === 'tag') {
    return pickVersionByTag(pkg, dep.spec)
  }
  return pickVersionByVersionRange(pkg, dep.spec, latestTag)
}

function pickVersionByTag(pkg: PackageDocument, tag: string) {
  const tagVersion = pkg['dist-tags'][tag]
  if (pkg.versions[tagVersion]) {
    return pkg.versions[tagVersion]
  }
  return null
}

function pickVersionByVersionRange(pkg: PackageDocument, versionRange: string, latestTag: string) {
  const latest = pkg['dist-tags'][latestTag]
  if (semver.satisfies(latest, versionRange, true)) {
    return pkg.versions[latest]
  }
  const versions = Object.keys(pkg.versions)
  const maxVersion = semver.maxSatisfying(versions, versionRange, true)
  if (maxVersion) {
    return pkg.versions[maxVersion]
  }
  return null
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
    name = '@' + enc(spec.name.substr(1))
  } else {
    name = enc(spec.name)
  }

  return url.resolve(registryUrl(spec.scope), name)
}
