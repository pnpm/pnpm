import getTarballName from './getTarballName'
import crypto = require('crypto')
import {PackageToResolve} from '../resolve'

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

export default function resolveTarball (pkg: PackageToResolve) {
  const name = getTarballName(pkg.rawSpec)

  return Promise.resolve({
    name,
    fullname: name + '#' + hash(pkg.rawSpec),
    dist: {
      tarball: pkg.rawSpec
    }
  })
}

function hash (str: string) {
  const hash = crypto.createHash('sha1')
  hash.update(str)
  return hash.digest('hex')
}
