var join = require('path').join
var url = require('url')
var enc = global.encodeURIComponent
var got = require('../got')
var registryUrl = require('registry-url')

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
    .then(url => {
      if (log) log('resolving')
      return got.get(url).then(promise => {
        return promise
      })
    })
    .then(res => JSON.parse(res.body))
    .then(res => {
      return {
        name: res.name,
        fullname: '' + res.name.replace('/', '!') + '@' + res.version,
        version: res.version, // used for displaying
        dist: res.dist
      }
    })
    .catch(errify)

  function errify (err) {
    if (err.statusCode === 404) {
      throw new Error("Module '" + pkg.raw + "' not found")
    }
    throw err
  }
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
