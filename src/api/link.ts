import path = require('path')
import readPkgUp = require('read-pkg-up')
import relSymlink from '../fs/rel_symlink'
import installPkgDeps from './install_pkg_deps'
import resolveGlobalPkgPath from '../resolve_global_pkg_path'
import {linkPkgBins} from '../install/link_bins'
import mkdirp from '../fs/mkdirp'
import {PublicInstallationOptions} from './install'

export function linkFromRelative (linkTo: string, opts: PublicInstallationOptions) {
  const cwd = opts && opts.cwd || process.cwd()
  const linkedPkgPath = path.resolve(cwd, linkTo)
  const currentModules = path.resolve(cwd, 'node_modules')
  return installPkgDeps(Object.assign({}, opts, { cwd: linkedPkgPath }))
    .then(() => mkdirp(currentModules))
    .then(() => readPkgUp({ cwd: linkedPkgPath }))
    .then(pkg => relSymlink(linkedPkgPath, path.resolve(currentModules, pkg.pkg.name)))
    .then(() => linkPkgBins(currentModules, linkedPkgPath))
}

export function linkFromGlobal (pkgName: string, opts: PublicInstallationOptions) {
  const globalPkgPath = resolveGlobalPkgPath(opts.globalPath)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  return linkFromRelative(linkedPkgPath, opts)
}

export function linkToGlobal (opts: PublicInstallationOptions) {
  const globalPkgPath = resolveGlobalPkgPath(opts.globalPath)
  const cwd = opts.cwd || process.cwd()
  return linkFromRelative(cwd, Object.assign({
    cwd: globalPkgPath
  }, opts))
}
