import path = require('path')
import normalizePath = require('normalize-path')
import fs = require('mz/fs')
import mkdirp = require('mkdirp-promise')
import readPkg from '../fs/readPkg'
import getPkgDirs from '../fs/getPkgDirs'
import binify from '../binify'
import isWindows = require('is-windows')
import cmdShim = require('@zkochan/cmd-shim')
import {Package} from '../types'
import logger from 'pnpm-logger'
import Module = require('module')
import union = require('lodash.union')

const IS_WINDOWS = isWindows()

export default async function linkAllBins (modules: string, binPath: string, exceptPkgName?: string) {
  const pkgDirs = await getPkgDirs(modules)
  return Promise.all(
    pkgDirs
      .map(pkgDir => normalizePath(pkgDir))
      .filter(pkgDir => !exceptPkgName || !pkgDir.endsWith(`/${exceptPkgName}`))
      .map((pkgDir: string) => linkPkgBins(pkgDir, binPath))
  )
}

/**
 * Links executable into `node_modules/.bin`.
 */
export async function linkPkgBins (target: string, binPath: string) {
  const pkg = await safeReadPkg(target)

  if (!pkg) {
    logger.warn(`There's a directory in node_modules without package.json: ${target}`)
    return
  }

  if (!pkg.bin) return

  const bins = binify(pkg)

  await mkdirp(binPath)
  await Promise.all(Object.keys(bins).map(async function (bin) {
    const externalBinPath = path.join(binPath, bin)
    const actualBin = bins[bin]
    const targetPath = path.join(target, actualBin)

    const nodePath = (await getBinNodePaths(target)).join(path.delimiter)
    return cmdShim(targetPath, externalBinPath, {nodePath})
  }))
}

async function getBinNodePaths (target: string) {
  const targetRealPath = await fs.realpath(target)

  return union(
    Module._nodeModulePaths(targetRealPath),
    Module._nodeModulePaths(target)
  )
}

async function safeReadPkg (pkgPath: string): Promise<Package | null> {
  try {
    return await readPkg(pkgPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return null
  }
}
