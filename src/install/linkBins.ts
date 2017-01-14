import path = require('path')
import normalizePath = require('normalize-path')
import linkDir from 'link-dir'
import fs = require('mz/fs')
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import getPkgDirs from '../fs/getPkgDirs'
import binify from '../binify'
import isWindows = require('is-windows')
import cmdShim = require('@zkochan/cmd-shim')
import {Package} from '../types'
import logger from '../logger'

const IS_WINDOWS = isWindows()

export default async function linkAllBins (modules: string, preserveSymlinks: boolean) {
  const pkgDirs = await getPkgDirs(modules)
  return Promise.all(pkgDirs.map((pkgDir: string) => linkPkgBins(modules, pkgDir, preserveSymlinks)))
}

/**
 * Links executable into `node_modules/.bin`.
 *
 * @param {String} modules - the node_modules path
 * @param {String} target - where the module is now; read package.json from here
 *
 * @example
 *     module = 'project/node_modules'
 *     target = 'project/node_modules/.store/rimraf@2.5.1'
 *     linkPkgBins(module, target)
 *
 *     // node_modules/.bin/rimraf -> ../.store/rimraf@2.5.1/cmd.js
 */
export async function linkPkgBins (modules: string, target: string, preserveSymlinks: boolean) {
  const pkg = await safeRequireJson(path.join(target, 'package.json'))

  if (!pkg) {
    logger.warn(`There's a directory in node_modules without package.json: ${target}`)
    return
  }

  if (!pkg.bin) return

  const bins = binify(pkg)
  const binDir = path.join(modules, '.bin')

  await mkdirp(binDir)
  await Promise.all(Object.keys(bins).map(async function (bin) {
    const externalBinPath = path.join(binDir, bin)
    const actualBin = bins[bin]
    const targetPath = path.join(target, actualBin)

    if (!preserveSymlinks) {
      const nodePath = getNodePaths(targetPath).join(path.delimiter)
      return cmdShim(targetPath, externalBinPath, {preserveSymlinks, nodePath})
    }

    const relTargetPath = normalizePath(path.join('..', pkg.name, actualBin))
    if (IS_WINDOWS) {
      const proxyFilePath = path.join(binDir, `${bin}.proxy`)
      await fs.writeFile(proxyFilePath, `#!/usr/bin/env node\r\nrequire("${relTargetPath}")`, 'utf8')
      return cmdShim(proxyFilePath, externalBinPath, {preserveSymlinks})
    }

    return proxy(externalBinPath, relTargetPath)
  }))
}

function getNodePaths (filename: string): string[] {
  const next = path.join(filename, '..')
  const modules = path.join(filename, 'node_modules')
  if (filename === next) return [modules]
  return [modules].concat(getNodePaths(next))
}

function makeExecutable (filePath: string) {
  return fs.chmod(filePath, 0o755)
}

async function proxy (proxyPath: string, relTargetPath: string) {
  // NOTE: this will be used only on non-windows
  // Hence, the \n line endings should be used
  const proxyContent = '#!/bin/sh\n' +
    '":" //# comment; exec /usr/bin/env node --preserve-symlinks "$0" "$@"\n' +
    `require("${relTargetPath}")`
  await fs.writeFile(proxyPath, proxyContent, 'utf8')
  return makeExecutable(proxyPath)
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
