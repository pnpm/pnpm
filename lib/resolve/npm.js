var url = require('url')
var enc = global.encodeURIComponent
var got = require('../got')
var pkgFullName = require('../pkg_full_name')
var registryUrl = require('registry-url')
var semver = require('semver')

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

module.exports = function resolveNpm (pkg, log) {
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  return Promise.resolve()
    .then(_ => toUri(pkg))
    .then(url => got.get(url).then(res => {
      if (log) log('resolving')
      return res.promise
    }))
    .then(res => JSON.parse(res.body))
    .then(res => pickVersionFromRegistryDocument(res, pkg))
    .then(res => ({
      name: res.name,
      fullname: pkgFullName(res),
      version: res.version, // used for displaying
      dist: res.dist
    }))
    .catch(err => errify(err, pkg))
}

function errify (err, pkg) {
  if (err.statusCode === 404) {
    throw new Error("Module '" + pkg.raw + "' not found")
  }
  throw err
}

function pickVersionFromRegistryDocument (pkg, dep) {
  var versions = Object.keys(pkg.versions)

  if (dep.type === 'tag') {
    var tagVersion = pkg['dist-tags'][dep.spec]
    if (pkg.versions[tagVersion]) {
      return pkg.versions[tagVersion]
    }
  } else {
    var maxVersion = semver.maxSatisfying(versions, dep.spec)
    if (maxVersion) {
      return pkg.versions[maxVersion]
    }
  }

  var message = versions.length
              ? 'Versions in registry:\n' + versions.join(', ') + '\n'
              : 'No valid version found.'
  var er = new Error('No compatible version found: ' +
                     dep.raw + '\n' + message)
  throw er
}

/**
 * Converts package data (from `npa()`) to a URI
 *
 *     toUri({ name: 'rimraf', rawSpec: '2' })
 *     // => 'https://registry.npmjs.org/rimraf/2'
 */

function toUri (pkg) {
  var name

  if (pkg.name.substr(0, 1) === '@') {
    name = '@' + enc(pkg.name.substr(1))
  } else {
    name = enc(pkg.name)
  }

  return url.resolve(registryUrl(pkg.scope), name)
}
