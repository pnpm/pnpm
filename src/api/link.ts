import path = require('path')
import loadJsonFile = require('load-json-file')
import symlinkDir = require('symlink-dir')
import logger, {streamParser} from '@pnpm/logger'
import {install} from './install'
import pathAbsolute = require('path-absolute')
import {linkPkgBins} from '../link/linkBins'
import extendOptions, {
  InstallOptions,
} from './extendInstallOptions'

const linkLogger = logger('link')

export default async function link (
  linkFrom: string,
  destModules: string,
  maybeOpts: InstallOptions & {
    skipInstall?: boolean,
    linkToBin?: string,
  }
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)

  if (!maybeOpts || !maybeOpts.skipInstall) {
    await install({
      ...opts,
      prefix: linkFrom,
      bin: path.join(linkFrom, 'node_modules', '.bin'),
      global: false,
    })
  }

  await linkToModules(linkFrom, destModules)

  const linkToBin = maybeOpts && maybeOpts.linkToBin || path.join(destModules, '.bin')
  await linkPkgBins(linkFrom, linkToBin)

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
  maybeOpts: InstallOptions & {globalPrefix: string}
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  await link(linkedPkgPath, path.join(linkTo, 'node_modules'), opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

export async function linkToGlobal (
  linkFrom: string,
  maybeOpts: InstallOptions & {
    globalPrefix: string,
    globalBin: string,
  }
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  await link(linkFrom, path.join(globalPkgPath, 'node_modules'), {
    ...opts,
    linkToBin: maybeOpts.globalBin,
  })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
