import resolveFromGit from '@pnpm/git-resolver'
import resolveFromNpm, {PackageMeta} from '@pnpm/npm-resolver'
import {PackageJson} from '@pnpm/types'
import {LoggedPkg} from '../loggers'
import {Got} from '../network/got'
import resolveFromLocal from './local'
import resolveFromTarball from './tarball'

export {PackageMeta}

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: undefined,
  tarball: string,
  integrity?: string,
  // needed in some cases to get the auth token
  // sometimes the tarball URL is under a different path
  // and the auth token is specified for the registry only
  registry?: string,
}

/**
 * directory on a file system
 */
export interface DirectoryResolution {
  type: 'directory',
  directory: string,
}

/**
 * Git repository
 */
export interface GitRepositoryResolution {
  type: 'git',
  repo: string,
  commit: string,
}

export type Resolution =
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution

export interface ResolveResult {
  id: string,
  resolution: Resolution,
  package?: PackageJson,
  latest?: string,
  normalizedPref?: string, // is null for npm-hosted dependencies
}

export interface ResolveOptions {
  loggedPkg: LoggedPkg,
  storePath: string,
  registry: string,
  metaCache: Map<string, PackageMeta>,
  prefix: string,
  offline: boolean,
  getJson<T> (url: string, registry: string): Promise<T>,
}

export interface WantedDependency {
  alias?: string,
  pref: string,
}

/**
 * Resolves a package in the NPM registry. Done as part of `install()`.
 *
 * @example
 *     var npa = require('npm-package-arg')
 *     resolve(npa('rimraf@2'))
 *       .then((res) => {
 *         res.id == 'rimraf@2.5.1'
 *         res.dist == {
 *           shasum: '0a1b2c...'
 *           tarball: 'http://...'
 *         }
 *       })
 */
export default async function (
  wantedDependency: WantedDependency,
  opts: ResolveOptions,
): Promise<ResolveResult> {
  const resolution = await resolveFromNpm(wantedDependency, opts)
    || await resolveFromTarball(wantedDependency, opts)
    || await resolveFromGit(wantedDependency, opts)
    || await resolveFromLocal(wantedDependency, opts)
  if (resolution) return resolution
  throw new Error(`Cannot resolve ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref} packages not supported`)
}
