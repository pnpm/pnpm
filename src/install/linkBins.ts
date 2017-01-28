import path = require('path')
import normalizePath = require('normalize-path')
import fs = require('mz/fs')
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import getPkgDirs from '../fs/getPkgDirs'
import binify from '../binify'
import isWindows = require('is-windows')
import cmdShim = require('@zkochan/cmd-shim')
import {Package} from '../types'
import logger from 'pnpm-logger'

const IS_WINDOWS = isWindows()

export default async function linkAllBins (modules: string, binPath: string) {
  const pkgDirs = await getPkgDirs(modules)
  return Promise.all(pkgDirs.map((pkgDir: string) => linkPkgBins(pkgDir, binPath)))
}

/**
 * Links executable into `node_modules/.bin`.
 */
export async function linkPkgBins (target: string, binPath: string) {
  const pkg = await safeRequireJson(path.join(target, 'package.json'))
  const targetRealPath = await fs.realpath(target)
  const extraNodePath = [path.join(targetRealPath, 'node_modules'), path.join(targetRealPath, '..', 'node_modules')]

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

    const nodePath = extraNodePath.concat(getNodePaths(targetPath)).join(path.delimiter)
    return cmdShim(targetPath, externalBinPath, {nodePath})
  }))
}

function getNodePaths (filename: string): string[] {
  const next = path.join(filename, '..')
  const modules = path.join(filename, 'node_modules')
  if (filename === next) return [modules]
  return [modules].concat(getNodePaths(next))
}

/**
 * Like `require()`, but returns `null` when it is not found
 */
function safeRequireJson (pkgJsonPath: string): Package | null {
  try {
    return requireJson(pkgJsonPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return null
  }
}
