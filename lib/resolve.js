var npa = require('npm-package-arg')
var join = require('path').join
var url = require('url')
var enc = global.encodeURIComponent
var got = require('./got')
var config = require('./config')

/**
 * Resolves a package in the NPM registry. Done as part of `install()`.
 *
 *     resolve('rimraf@2')
 *       .then((res) => {
 *         res.name == 'rimraf'
 *         res.version == '2.5.1'
 *         res.dist == {
 *           shasum: '0a1b2c...'
 *           tarball: 'http://...'
 *         }
 *       })
 */

module.exports = function resolve (pkgSpec) {
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  return Promise.resolve()
    .then(_ => toUri(npa(pkgSpec)))
    .then(url => got(url))
    .then(res => JSON.parse(res.body))
}

/**
 * Converts package data (from `npa()`) to a URI
 *
 *     toUri({ name: 'rimraf', rawSpec: '2' })
 *     // => 'https://registry.npmjs.org/rimraf/2'
 */

function toUri (pkgData) {
  // TODO: handle scoped packages
  if (pkgData.rawSpec.length) {
    uri = join(enc(pkgData.name), enc(pkgData.rawSpec))
  } else {
    uri = enc(pkgData.name)
  }

  return url.resolve(config.registry, uri)
}
