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
  const resolution: Resolution = {
    type: 'tarball',
    tarball: spec.rawSpec,
  }

  return {
    id: spec.rawSpec
      .replace(/^.*:\/\/(git@)?/, '')
      .replace(/\.tgz$/, ''),
    resolution,
  }
}
