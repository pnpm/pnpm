import url = require('url')
const enc = encodeURIComponent
import createPkgId from './createPkgId'
import registryUrl = require('registry-url')
import semver = require('semver')
import {ResolveOptions, ResolveResult} from '.'
import {Package} from '../api/initCmd'
import {PackageSpec} from '../install'
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
    const correctPkg = pickVersionFromRegistryDocument(parsedBody, spec)
    return {
      id: createPkgId(correctPkg),
      fetch: createRemoteTarballFetcher({
        shasum: correctPkg.dist.shasum,
        tarball: correctPkg.dist.tarball
      })
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

function pickVersionFromRegistryDocument (pkg: PackageDocument, dep: PackageSpec) {
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
