'use strict'
const resolveNpm = require('./resolve/npm')
const resolveTarball = require('./resolve/tarball')
const resolveGithub = require('./resolve/github')
const resolveLocal = require('./resolve/local')

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

module.exports = function resolve (pkg, opts) {
  if (pkg.type === 'range' || pkg.type === 'version' || pkg.type === 'tag') {
    return resolveNpm(pkg, opts)
  } else if (pkg.type === 'remote') {
    return resolveTarball(pkg, opts)
  } else if (pkg.type === 'hosted' && pkg.hosted.type === 'github') {
    return resolveGithub(pkg, opts)
  } else if (pkg.type === 'local') {
    return resolveLocal(pkg, opts)
  } else {
    throw new Error('' + pkg.rawSpec + ': ' + pkg.type + ' packages not supported')
  }
}
