import resolveFromNpm, {PackageMeta} from './npm'
import resolveFromTarball from './tarball'
import resolveFromLocal from './local'
import resolveFromGit from './git'
import {Got} from '../network/got'
import {Package} from '../types'
import {LoggedPkg} from 'pnpm-logger'

export {PackageMeta}

/**
 * tarball hosted remotely
 */
export type TarballResolution = {
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
export type DirectoryResolution = {
  type: 'directory',
  directory: string,
}

/**
 * Git repository
 */
export type GitRepositoryResolution = {
  type: 'git',
  repo: string,
  commit: string,
}

export type Resolution =
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution

export type ResolveResult = {
  id: string,
  resolution: Resolution,
  package?: Package,
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
  }
}

export type RegistryPackageSpec = PackageSpecBase & {
  type: 'tag' | 'version' | 'range',
  registry: true,
}

export type PackageSpecBase = {
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

export type ResolveOptions = {
  loggedPkg: LoggedPkg,
  got: Got,
  storePath: string,
  registry: string,
  metaCache: Map<string, PackageMeta>,
  prefix: string,
  offline: boolean,
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
      throw new Error(`${spec['rawSpec']}: ${spec['type']} packages not supported`)
  }
}
