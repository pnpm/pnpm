import path = require('path')
import normalizePath = require('normalize-path')
import linkDir from 'link-dir'
import fs = require('mz/fs')
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import getPkgDirs from '../fs/getPkgDirs'
import binify from '../binify'
import {isWindows, preserveSymlinks} from '../env'
import cbCmdShim = require('@zkochan/cmd-shim')
import thenify = require('thenify')
import {Package} from '../types'
const cmdShim = thenify(cbCmdShim)

export default async function linkAllBins (modules: string) {
  const pkgDirs = await getPkgDirs(modules)
  return Promise.all(pkgDirs.map((pkgDir: string) => linkPkgBins(modules, pkgDir)))
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
export async function linkPkgBins (modules: string, target: string) {
  const pkg = await safeRequireJson(path.join(target, 'package.json'))

  if (!pkg) {
    console.warn(`There's a directory in node_modules without package.json: ${target}`)
    return
  }

  if (!pkg.bin) return

  const bins = binify(pkg)
  const binDir = path.join(modules, '.bin')

  await mkdirp(binDir)
  await Promise.all(Object.keys(bins).map(async function (bin) {
    const actualBin = bins[bin]
    const externalBinPath = path.join(binDir, bin)

    const targetPath = path.join(pkg.name, actualBin)
    const normalTargetPath = normalizePath(targetPath)
    if (isWindows) {
      if (!preserveSymlinks) {
        return cmdShim(path.join(target, actualBin), externalBinPath, {preserveSymlinks})
      }
      const proxyFilePath = path.join(binDir, `${bin}.proxy`)
      await fs.writeFile(proxyFilePath, `#!/usr/bin/env node\r\nrequire("../${normalTargetPath}")`, 'utf8')
      return cmdShim(proxyFilePath, externalBinPath, {preserveSymlinks})
    }

    if (!preserveSymlinks) {
      await makeExecutable(path.join(target, actualBin))
      return linkDir(
        path.join(target, actualBin),
        externalBinPath)
    }

    return proxy(externalBinPath, targetPath)
  }))
}

function makeExecutable (filePath: string) {
  return fs.chmod(filePath, 0o755)
}

async function proxy (proxyPath: string, targetPath: string) {
  // NOTE: this will be used only on non-windows
  // Hence, the \n line endings should be used
  const proxyContent = '#!/bin/sh\n' +
    '":" //# comment; exec /usr/bin/env node --preserve-symlinks "$0" "$@"\n' +
    `require("../${targetPath}")`
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
