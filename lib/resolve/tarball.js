'use strict'
const getTarballName = require('./get_tarball_name')
const crypto = require('crypto')

/**
 * Resolves a 'remote' package.
 *
 *     pkg = {
 *       raw: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
 *       scope: null,
 *       name: null,
 *       rawSpec: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
 *       spec: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
 *       type: 'remote' }
 *     resolveTarball(pkg)
 */

module.exports = function resolveTarball (pkg) {
  const name = getTarballName(pkg.rawSpec)

  return Promise.resolve({
    name,
    fullname: name + '#' + hash(pkg.rawSpec),
    dist: {
      tarball: pkg.rawSpec
    }
  })
}

function hash (str) {
  const hash = crypto.createHash('sha1')
  hash.update(str)
  return hash.digest('hex')
}
