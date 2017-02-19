import path = require('path')
import readPkg = require('read-pkg')
import symlinkDir from 'symlink-dir'
import logger from 'pnpm-logger'
import {install} from './install'
import expandTilde from '../fs/expandTilde'
import {linkPkgBins} from '../install/linkBins'
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'

const linkLogger = logger('link')

export default async function link (
  linkFrom: string,
  linkTo: string,
  maybeOpts?: PnpmOptions
) {
  const opts = extendOptions(maybeOpts)

  await install(Object.assign({}, opts, { cwd: linkFrom }))

  const destModules = path.join(linkTo, 'node_modules')
  await linkToModules(linkFrom, destModules)

  const bin = path.join(destModules, '.bin')
  await linkPkgBins(linkFrom, bin)
}

async function linkToModules (linkFrom: string, modules: string) {
  const pkg = await readPkg(linkFrom)
  const dest = path.join(modules, pkg.name)
  linkLogger.info(`${dest} -> ${linkFrom}`)
  await symlinkDir(linkFrom, dest)
}

export async function linkFromGlobal (
  pkgName: string,
  linkTo: string,
  maybeOpts?: PnpmOptions
) {
  const opts = extendOptions(maybeOpts)
  const globalPkgPath = expandTilde(opts.globalPath)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  await link(linkedPkgPath, linkTo, opts)
}

export async function linkToGlobal (
  linkFrom: string,
  maybeOpts?: PnpmOptions
) {
  const opts = extendOptions(maybeOpts)
  opts.global = true // bins will be linked to the global bin path
  const globalPkgPath = expandTilde(opts.globalPath)
  await link(linkFrom, globalPkgPath, opts)
}
