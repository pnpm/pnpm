import getTarballName from './getTarballName'
import crypto = require('crypto')
import {PackageSpec, ResolveResult, ResolveOptions} from '.'

/**
 * Resolves a 'remote' package.
 *
 * @example
 *     pkg = {
 *       raw: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
 *       scope: null,
 *       name: null,
 *       rawSpec: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
 *       spec: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
 *       type: 'remote' }
 *     resolveTarball(pkg)
 */
export default function resolveTarball (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const name = getTarballName(spec.rawSpec)

  return Promise.resolve({
    id: name + '#' + hash(spec.rawSpec),
    tarball: spec.rawSpec,
  })
}

function hash (str: string) {
  const hash = crypto.createHash('sha1')
  hash.update(str)
  return hash.digest('hex')
}
