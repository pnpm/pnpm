import {PackageJson} from '@pnpm/types'
import {LoggedPkg} from '../loggers'
import {Got} from '../network/got'
import resolveFromGit from './git'
import resolveFromLocal from './local'
import resolveFromNpm, {PackageMeta} from './npm'
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
}

export type HostedPackageSpec = PackageSpecBase & {
  type: 'git',
  registry: false,
  gitCommittish: string,
  hosted?: {
    type: string,
    shortcut: string,
    sshUrl: string,
    user: string,
    project: string,
    committish: string,
  },
}

export type RegistryPackageSpec = PackageSpecBase & {
  type: 'tag' | 'version' | 'range',
  registry: true,
}

export interface PackageSpecBase {
  raw: string,
  rawSpec: string
  name: string,
  scope: string,
  saveSpec: string,
  fetchSpec: string,
  dev: boolean,
  optional: boolean,
}

export type PackageSpec = HostedPackageSpec |
  RegistryPackageSpec |
  PackageSpecBase & {
    type: 'directory' | 'file' | 'remote',
    registry: false,
  }

export interface ResolveOptions {
  loggedPkg: LoggedPkg,
  got: Got,
  storePath: string,
  registry: string,
  metaCache: Map<string, PackageMeta>,
  prefix: string,
  offline: boolean,
  downloadPriority: number,
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
export default async function (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  switch (spec.type) {
    case 'range':
    case 'version':
    case 'tag':
      return resolveFromNpm(spec, opts)
    case 'remote':
      return resolveFromTarball(spec, opts)
    case 'directory':
    case 'file':
      return resolveFromLocal(spec, opts)
    case 'git':
      return resolveFromGit(spec, opts)
    default:
      // tslint:disable-next-line
      throw new Error(`${spec['rawSpec']}: ${spec['type']} packages not supported`)
  }
}
