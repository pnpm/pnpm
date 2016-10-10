import resolveFromNpm from './npm'
import resolveFromTarball from './tarball'
import resolveFromGithub from './github'
import resolveFromLocal from './local'
import resolveFromGit from './git'
import {PackageSpec} from '../install'
import {Got} from '../network/got'
import {FetchOptions} from './fetch'

export type ResolveResult = {
  id: string,
  fetch(target: string, opts: FetchOptions): Promise<void>,
  root?: string
}

export type HostedPackageSpec = PackageSpec & {
  hosted: {
    type: string,
    shortcut: string,
    sshUrl: string
  }
}

export type ResolveOptions = {
  log(msg: string): void,
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
      return resolveFromTarball(spec)
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
