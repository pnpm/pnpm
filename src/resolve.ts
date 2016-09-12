import resolveNpm from './resolve/npm'
import resolveTarball from './resolve/tarball'
import resolveGithub from './resolve/github'
import resolveLocal from './resolve/local'
import {PackageSpec} from './install'
import {Got} from './network/got'

export type PackageDist = {
  local: boolean,
  remove: boolean,
  tarball: string,
  shasum: string
}

export type ResolveResult = {
  name: string,
  fullname: string,
  version: string,
  dist: PackageDist,
  root?: string
}

export type PackageToResolve = PackageSpec & {
  root: string
}

export type HostedPackageToResolve = PackageToResolve & {
  hosted: {
    type: string,
    shortcut: string
  }
}

export type ResolveOptions = {
  log(msg: string): void,
  got: Got
}

/**
 * Resolves a package in the NPM registry. Done as part of `install()`.
 *
 *     var npa = require('npm-package-arg')
 *     resolve(npa('rimraf@2'))
 *       .then((res) => {
 *         res.fullname == 'rimraf@2.5.1'
 *         res.dist == {
 *           shasum: '0a1b2c...'
 *           tarball: 'http://...'
 *         }
 *       })
 */

export default function resolve (pkg: PackageToResolve, opts: ResolveOptions): Promise<ResolveResult> {
  if (pkg.type === 'range' || pkg.type === 'version' || pkg.type === 'tag') {
    return resolveNpm(pkg, opts)
  } else if (pkg.type === 'remote') {
    return resolveTarball(pkg)
  } else if (pkg.type === 'hosted' && (<HostedPackageToResolve>pkg).hosted.type === 'github') {
    return resolveGithub(<HostedPackageToResolve>pkg, opts)
  } else if (pkg.type === 'local') {
    return resolveLocal(pkg)
  } else {
    throw new Error('' + pkg.rawSpec + ': ' + pkg.type + ' packages not supported')
  }
}
