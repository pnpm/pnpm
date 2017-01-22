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
import logger from 'pnpm-logger'
import os = require('os')
import {SwitcherOptions} from '../switcher'

export type BinOptions = {
  preserveSymlinks: boolean,
  global: boolean,
}

const IS_WINDOWS = isWindows()

export default async function linkAllBins (modules: string, binPath: string, opts: BinOptions) {
  const pkgDirs = await getPkgDirs(modules)
  return Promise.all(pkgDirs.map((pkgDir: string) => linkPkgBins(pkgDir, binPath, opts)))
}

/**
 * Links executable into `node_modules/.bin`.
 */
export async function linkPkgBins (target: string, binPath: string, opts: BinOptions) {
  const pkg = await safeRequireJson(path.join(target, 'package.json'))

  if (!pkg) {
    logger.warn(`There's a directory in node_modules without package.json: ${target}`)
    return
  }

  if (!pkg.bin) return

  const bins = binify(pkg)

  await mkdirp(binPath)
  await Promise.all(Object.keys(bins).map(bin => linkBin(target, bin, bins[bin], binPath, pkg.name, opts)))
}

async function linkBin (
  target: string,
  bin: string,
  actualBin: string,
  binPath: string,
  pkgName: string,
  opts: BinOptions
) {
  const preserveSymlinks = opts.preserveSymlinks
  const externalBinPath = path.join(binPath, bin)
  const targetPath = path.join(target, actualBin)
  const relativeRequirePath = await getBinRequirePath(binPath, targetPath)

  if (!preserveSymlinks) {
    if (opts.global) {
      const proxyFilePath = path.join(binPath, `${bin}.switch`)
      const switcherOptions: SwitcherOptions = {
        requiredBin: path.join(pkgName, actualBin),
        relativeRequirePath,
        bin,
      }
      const switcherRequirePath = await getBinRequirePath(binPath, path.join(__dirname, '..', '..', 'lib', 'switcher'))
      await fs.writeFile(proxyFilePath, '#!/usr/bin/env node' +
        os.EOL + `require('${switcherRequirePath}').default(${JSON.stringify(switcherOptions)})`, 'utf8')
      return cmdShim(proxyFilePath, externalBinPath, {preserveSymlinks})
    }
    const nodePath = getNodePaths(targetPath).join(path.delimiter)
    return cmdShim(targetPath, externalBinPath, {preserveSymlinks, nodePath})
  }

  if (IS_WINDOWS) {
    const proxyFilePath = path.join(binPath, `${bin}.proxy`)
    await fs.writeFile(proxyFilePath, `#!/usr/bin/env node\r\nrequire("${relativeRequirePath}")`, 'utf8')
    return cmdShim(proxyFilePath, externalBinPath, {preserveSymlinks})
  }

  return proxy(externalBinPath, relativeRequirePath)
}

async function getBinRequirePath (binPath: string, targetPath: string) {
  const realBinPath = await fs.realpath(binPath)
  const relTargetPath = normalizePath(path.relative(realBinPath, targetPath))
  if (!relTargetPath.startsWith('.')) {
    // require should always be identified as relative by Node
    return `./${relTargetPath}`
  }
  return relTargetPath
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

async function proxy (proxyPath: string, relativeRequirePath: string): Promise<void> {
  // NOTE: this will be used only on non-windows
  // Hence, the \n line endings should be used
  const proxyContent = '#!/bin/sh\n' +
    '":" //# comment; exec /usr/bin/env node --preserve-symlinks "$0" "$@"\n' +
    `require("${relativeRequirePath}")`
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
