import path = require('path')
import loadJsonFile = require('load-json-file')
import symlinkDir = require('symlink-dir')
import logger, {streamParser} from 'pnpm-logger'
import {install} from './install'
import pathAbsolute = require('path-absolute')
import {linkPkgBins} from '../link/linkBins'
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'

const linkLogger = logger('link')

export default async function link (
  linkFrom: string,
  linkTo: string,
  maybeOpts?: PnpmOptions & {skipInstall?: boolean}
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)

  if (!maybeOpts || !maybeOpts.skipInstall) {
    await install(Object.assign({}, opts, { prefix: linkFrom, global: false }))
  }

  const destModules = path.join(linkTo, 'node_modules')
  await linkToModules(linkFrom, destModules)

  const bin = opts.bin || path.join(destModules, '.bin')
  await linkPkgBins(linkFrom, bin)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function linkToModules (linkFrom: string, modules: string) {
  const pkg = await loadJsonFile(path.join(linkFrom, 'package.json'))
  const dest = path.join(modules, pkg.name)
  linkLogger.info(`${dest} -> ${linkFrom}`)
  await symlinkDir(linkFrom, dest)
}

export async function linkFromGlobal (
  pkgName: string,
  linkTo: string,
  maybeOpts: PnpmOptions & {globalPrefix: string}
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  await link(linkedPkgPath, linkTo, opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

export async function linkToGlobal (
  linkFrom: string,
  maybeOpts: PnpmOptions & {
    globalPrefix: string,
    globalBin: string,
  }
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  opts.global = true // bins will be linked to the global bin path
  opts.bin = maybeOpts.globalBin
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  await link(linkFrom, globalPkgPath, opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
