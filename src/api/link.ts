import path = require('path')
import readPkgUp = require('read-pkg-up')
import relSymlink from '../fs/relSymlink'
import installPkgDeps from './installPkgDeps'
import resolveGlobalPkgPath from '../resolveGlobalPkgPath'
import {linkPkgBins} from '../install/linkBins'
import mkdirp from '../fs/mkdirp'
import {PnpmOptions, StrictPnpmOptions} from '../types'
import defaults from '../defaults'

export async function linkFromRelative (linkTo: string, optsNullable: PnpmOptions) {
  const opts: StrictPnpmOptions = Object.assign({}, defaults, optsNullable)
  const cwd = opts && opts.cwd || process.cwd()
  const linkedPkgPath = path.resolve(cwd, linkTo)
  const currentModules = path.resolve(cwd, 'node_modules')
  await installPkgDeps(Object.assign({}, opts, { cwd: linkedPkgPath }))
  await mkdirp(currentModules)
  const pkg = await readPkgUp({ cwd: linkedPkgPath })
  await relSymlink(linkedPkgPath, path.resolve(currentModules, pkg.pkg.name))
  return linkPkgBins(currentModules, linkedPkgPath)
}

export function linkFromGlobal (pkgName: string, optsNullable: PnpmOptions) {
  const opts: StrictPnpmOptions = Object.assign({}, defaults, optsNullable)
  const globalPkgPath = resolveGlobalPkgPath(opts.globalPath)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  return linkFromRelative(linkedPkgPath, opts)
}

export function linkToGlobal (optsNullable: PnpmOptions) {
  const opts: StrictPnpmOptions = Object.assign({}, defaults, optsNullable)
  const globalPkgPath = resolveGlobalPkgPath(opts.globalPath)
  const cwd = opts.cwd || process.cwd()
  return linkFromRelative(cwd, Object.assign({
    cwd: globalPkgPath
  }, opts))
}
