var Promise = require('./promise')
var debug = require('debug')('cnpm:install')
var config = require('./config')
var url = require('url')
var got = require('./got')
var npa = require('npm-package-arg')
var join = require('path').join
var mkdirp = require('./mkdirp')
var fetch = require('./fetch')
var enc = global.encodeURIComponent

/*
 * Installs a package.
 *
 *     - resolve() - resolve from registry.npmjs.org
 *     - fetch() - download tarball into node_modules/.tmp/{uuid}
 *     - recurse into its dependencies
 *     - run postinstall hooks
 *     - move .tmp/{uuid} into node_modules/{name}@{version}
 *     - symlink node_modules/{name}
 *     - symlink bins
 */

function install (modPath, pkg, options) {
  debug('installing ' + pkg)

  return resolve(pkg)
    .then(function (res) {
      var name = '' + res.name + '@' + res.version
      return mkdirp(join(modPath, name))
        .then(function (_) { return fetch(_, res.dist.tarball, res.dist.shasum) })
    })
}

/**
 * Resolves a package in the NPM registry
 *
 * Returns { name, version }
 */

function resolve (pkg) {
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  return new Promise(function (resolve) {
    var pkgData = npa(pkg)
    var uri = toUri(pkgData)
    resolve(uri)
  })
    .then(got)
    .then(function (res) {
      var body = JSON.parse(res.body)
      return body
    })
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

/*
 * Export
 */

module.exports = {
  install: install,
  toUri: toUri
}
