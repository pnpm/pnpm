import getTarballName from './getTarballName'
import crypto = require('crypto')
import {PackageSpec, ResolveOptions, Resolution, ResolveResult} from '.'

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
export default async function resolveTarball (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const name = getTarballName(spec.rawSpec)

  const resolution: Resolution = {
    type: 'tarball',
    id: name + '#' + hash(spec.rawSpec),
    tarball: spec.rawSpec,
  }

  return {resolution}
}

function hash (str: string) {
  const hash = crypto.createHash('sha1')
  hash.update(str)
  return hash.digest('hex')
}
