import url = require('url')
const enc = encodeURIComponent
import pkgFullName from '../pkg_full_name'
import registryUrl = require('registry-url')
import semver = require('semver')
import {PackageToResolve, ResolveOptions, PackageDist, ResolveResult} from '../resolve'
import {Package} from '../api/init_cmd'

/**
 * Resolves a package in the NPM registry. Done as part of `install()`.
 *
 *     var npa = require('npm-package-arg')
 *     resolve(npa('rimraf@2'))
 *       .then((res) => {
 *         res.fullname == 'rimraf@2.5.1'
 *         res.dist == {
 *           shasum: '0a1b2c...'
 *           tarball: 'http://...'
 *         }
 *       })
 */

export default async function resolveNpm (pkg: PackageToResolve, opts: ResolveOptions): Promise<ResolveResult> {
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  try {
    const url = toUri(pkg)
    if (opts.log) opts.log('resolving')
    const res = await opts.got.get(url)
    const parsedBody = JSON.parse(res.body)
    const correctPkg = pickVersionFromRegistryDocument(parsedBody, pkg)
    return {
      name: correctPkg.name,
      fullname: pkgFullName(correctPkg),
      version: correctPkg.version, // used for displaying
      dist: correctPkg.dist
    }
  } catch (err) {
    if (err['statusCode'] === 404) {
      throw new Error("Module '" + pkg.raw + "' not found")
    }
    throw err
  }
}

type StringDict = {
  [name: string]: string
}

type PackageInRegistry = Package & {
  dist: PackageDist
}

type PackageDocument = {
  'dist-tag': StringDict,
  versions: {
    [name: string]: PackageInRegistry
  }
}

function pickVersionFromRegistryDocument (pkg: PackageDocument, dep: PackageToResolve) {
  const versions = Object.keys(pkg.versions)

  if (dep.type === 'tag') {
    const tagVersion = pkg['dist-tags'][dep.spec]
    if (pkg.versions[tagVersion]) {
      return pkg.versions[tagVersion]
    }
  } else {
    const maxVersion = semver.maxSatisfying(versions, dep.spec)
    if (maxVersion) {
      return pkg.versions[maxVersion]
    }
  }

  const message = versions.length
              ? 'Versions in registry:\n' + versions.join(', ') + '\n'
              : 'No valid version found.'
  const er = new Error('No compatible version found: ' +
                     dep.raw + '\n' + message)
  throw er
}

/**
 * Converts package data (from `npa()`) to a URI
 *
 *     toUri({ name: 'rimraf', rawSpec: '2' })
 *     // => 'https://registry.npmjs.org/rimraf/2'
 */

function toUri (pkg: PackageToResolve) {
  let name: string

  if (pkg.name.substr(0, 1) === '@') {
    name = '@' + enc(pkg.name.substr(1))
  } else {
    name = enc(pkg.name)
  }

  return url.resolve(registryUrl(pkg.scope), name)
}
