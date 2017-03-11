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
  shasum?: string,
}

/**
 * directory on a file system
 */
export type DirectoryResolution = {
  type: 'directory',
  root: string,
}

/**
 * Git repository
 */
export type GitRepositoryResolution = {
  type: 'git-repo',
  repo: string,
  commitId: string,
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

export type PackageSpec = {
  raw: string,
  name: string,
  scope: string,
  type: 'git' | 'hosted' | 'tag' | 'version' | 'range' | 'local' | 'remote',
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
  localRegistry: string,
  metaCache: Map<string, PackageMeta>,
  root: string,
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
    case 'local':
      return resolveFromLocal(spec, opts)
    case 'hosted':
    case 'git':
      return resolveFromGit(spec, opts)
    default:
      throw new Error(`${spec.rawSpec}: ${spec.type} packages not supported`)
  }
}
