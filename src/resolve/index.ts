import resolveFromNpm from './npm'
import resolveFromTarball from './tarball'
import resolveFromGithub from './github'
import resolveFromLocal from './local'
import resolveFromGit from './git'
import {Got} from '../network/got'
import {Package} from '../types'
import {LoggedPkg} from '../logging/logInstallStatus'

export type ResolveResult = {
  id: string,
  pkg?: Package,
  tarball?: string,
  shasum?: string
  root?: string,
  repo?: string,
  ref?: string,
  fetch?: (target: string) => Promise<void>,
}

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
export default function (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
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
      const hspec = <HostedPackageSpec>spec
      if (hspec.hosted.type === 'github' && !isSsh(hspec.spec)) {
        return resolveFromGithub(hspec, opts)
      }
      return resolveFromGit(spec, opts)
    case 'git':
      return resolveFromGit(spec, opts)
    default:
      throw new Error(`${spec.rawSpec}: ${spec.type} packages not supported`)
  }
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.substr(0, 10) === 'git+ssh://'
    || gitSpec.substr(0, 4) === 'git@'
}
