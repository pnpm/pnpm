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
  const name = getTarballName(pkg.raw)

  return Promise.resolve({
    name,
    fullname: name + '#' + hash(pkg.raw),
    dist: {
      tarball: pkg.raw
    }
  })
}

function hash (str) {
  const hash = crypto.createHash('sha1')
  hash.update(str)
  return hash.digest('hex')
}
