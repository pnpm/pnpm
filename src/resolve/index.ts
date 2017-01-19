import resolveFromNpm from './npm'
import resolveFromTarball from './tarball'
import resolveFromLocal from './local'
import resolveFromGit from './git'
import {Got} from '../network/got'
import {Package} from '../types'
import {LoggedPkg} from 'pnpm-logger'

export type ResolutionBase = {
  id: string,
}

/**
 * npm registry hosted package
 */
export type PackageResolution = ResolutionBase & {
  type: 'package',
  tarball: string,
  shasum?: string,
  pkg?: Package,
}

/**
 * tarball hosted remotely
 */
export type TarballResolution = ResolutionBase & {
  type: 'tarball',
  tarball: string,
  shasum?: string,
}

/**
 * directory on a file system
 */
export type DirectoryResolution = ResolutionBase & {
  type: 'directory',
  root: string,
  link?: boolean,
}

/**
 * Git repository
 */
export type GitRepositoryResolution = ResolutionBase & {
  type: 'git-repo',
  repo: string,
  commitId: string,
}

export type Resolution =
  PackageResolution |
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution

export type PackageSpec = {
  raw: string,
  name: string,
  scope: string,
  type: string,
  spec: string,
  rawSpec: string
}

export type HostedPackageSpec = PackageSpec & {
  hosted: {
    type: string,
    shortcut: string,
    sshUrl: string
  }
}

export type ResolveOptions = {
  loggedPkg: LoggedPkg,
  got: Got,
  root: string,
  linkLocal: boolean,
  tag: string
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
export default async function (spec: PackageSpec, opts: ResolveOptions): Promise<Resolution> {
  switch (spec.type) {
    case 'range':
    case 'version':
    case 'tag':
      return resolveFromNpm(spec, opts)
    case 'remote':
      return resolveFromTarball(spec, opts)
    case 'local':
      return resolveFromLocal(spec, opts)
    case 'hosted':
    case 'git':
      return resolveFromGit(spec, opts)
    default:
      throw new Error(`${spec.rawSpec}: ${spec.type} packages not supported`)
  }
}
