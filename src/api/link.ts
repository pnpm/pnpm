import path = require('path')
import readPkgUp = require('read-pkg-up')
import symlinkDir from 'symlink-dir'
import {install} from './install'
import expandTilde from '../fs/expandTilde'
import {linkPkgBins} from '../install/linkBins'
import mkdirp = require('mkdirp-promise')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'

export async function linkFromRelative (linkTo: string, maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const cwd = opts && opts.cwd || process.cwd()
  const linkedPkgPath = path.resolve(cwd, linkTo)
  const currentModules = path.resolve(cwd, 'node_modules')
  await install(Object.assign({}, opts, { cwd: linkedPkgPath }))
  await mkdirp(currentModules)
  const pkg = await readPkgUp({ cwd: linkedPkgPath })
  await symlinkDir(linkedPkgPath, path.resolve(currentModules, pkg.pkg.name))
  const bin = path.join(currentModules, '.bin')
  return linkPkgBins(linkedPkgPath, bin)
}

export function linkFromGlobal (pkgName: string, maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const globalPkgPath = expandTilde(opts.globalPath)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  return linkFromRelative(linkedPkgPath, opts)
}

export function linkToGlobal (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const globalPkgPath = expandTilde(opts.globalPath)
  return linkFromRelative(opts.cwd, Object.assign({}, opts, {
    cwd: globalPkgPath
  }))
}
