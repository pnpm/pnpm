var join = require('path').join
var url = require('url')
var enc = global.encodeURIComponent
var got = require('../got')
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
    .catch(err => err.statusCode === 404
      ? getAllVersionsAndMatchOnClient(pkg)
      : errify(err, pkg)
    )
    .then(res => ({
      name: res.name,
      fullname: '' + res.name.replace('/', '!') + '@' + res.version,
      version: res.version, // used for displaying
      dist: res.dist
    }))
}

function errify (err, pkg) {
  if (err.statusCode === 404) {
    throw new Error("Module '" + pkg.raw + "' not found")
  }
  throw err
}

function getAllVersionsAndMatchOnClient (pkg) {
  return Promise.resolve()
    .then(_ => url.resolve(registryUrl(pkg.scope), pkg.name))
    .then(url => got.get(url).then(res => res.promise))
    .then(res => JSON.parse(res.body))
    .then(res => pickVersionFromRegistryDocument(res, pkg))
    .catch(err => errify(err, pkg))
}

function pickVersionFromRegistryDocument (pkg, dep) {
  var versions = Object.keys(pkg.versions)
  if (dep.tag === 'tag' && dep.spec === 'latest') {
    var sortedVersions = versions.sort(semver.rcompare)
    return pkg(sortedVersions[0])
  }

  if (dep.type === 'tag' && dep.spec !== 'latest') {
    var tagVersion = pkg['dist-tags'][dep.spec]
    if (pkg.versions[tagVersion]) {
      return pkg.versions[tagVersion]
    }
  } else {
    var spec = scopeWorkarounds(dep)
    var maxSatisfyingVersion = semver.maxSatisfying(versions, spec)
    if (maxSatisfyingVersion) {
      return pkg.versions[maxSatisfyingVersion]
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
  // TODO: handle scoped packages
  var name, uri

  if (pkg.name.substr(0, 1) === '@') {
    name = '@' + enc(pkg.name.substr(1))
  } else {
    name = enc(pkg.name)
  }

  if (pkg.name.substr(0, 1) === '@') {
    uri = join(name, scopeWorkarounds(pkg))
  } else {
    uri = join(name, pkg.spec)
  }

  return url.resolve(registryUrl(pkg.scope), uri)
}

/*
 * The npm registry doesn't support resolutions of dist tags of scoped
 * packages, or exact versions... only ranges. This means you can't do `pnpm
 * install @rstacruz/tap-spec` or `pnpm install @rstacruz/tap-spec@latest`.
 *
 * As a workaround, we'll use `*` for `latest`. And for exact versions,
 * we'll convert them to a range (`=2.0.0`).
 *
 * OK   - https://registry.npmjs.org/@rstacruz%2Ftap-spec/*
 * Nope - https://registry.npmjs.org/@rstacruz%2Ftap-spec/latest
 */

function scopeWorkarounds (pkg) {
  if (pkg.type === 'tag' && pkg.spec === 'latest') {
    return '*'
  } else if (pkg.type === 'version') {
    return '=' + pkg.spec
  } else if (pkg.type === 'range') {
    return pkg.spec
  } else {
    throw new Error('Unsupported for now: ' + pkg.raw)
  }
}
