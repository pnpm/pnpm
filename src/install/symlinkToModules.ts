import path = require('path')
import fs = require('mz/fs')
import requireJson from '../fs/requireJson'
import linkDir from 'link-dir'
import mkdirp from '../fs/mkdirp'
import {Package} from '../types'
import thenify = require('thenify')
import cbcpr = require('cpr')
const cpr = thenify(cbcpr)

/**
 * Perform the final symlinking of ./.store/x@1.0.0 -> ./x.
 *
 * @example
 *     target = '/node_modules/.store/lodash@4.0.0'
 *     modules = './node_modules'
 *     symlinkToModules(target, modules)
 */
export default async function symlinkToModules (target: string, modules: string) {
  // TODO: uncomment to make things fail
  const pkgData = await requireJson(path.join(target, 'package.json'))
  if (!pkgData.name) { throw new Error('Invalid package.json for ' + target) }

  const out = path.join(modules, pkgData.name)

  // some action, like running lifecycle events,
  // cannot be done on a symlinked package
  if (pkgShouldBeCopied(pkgData)) {
    await cpr(target, out)
    return
  }

  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  await mkdirp(out)

  const dirs = await fs.readdir(target)
  await Promise.all(
    dirs
      .map((relativePath: string) => {
        if (relativePath === 'node_modules') return
        const absolutePath = path.join(target, relativePath)
        const dest = path.join(out, relativePath);
        if (fs.statSync(absolutePath).isDirectory()) {
          return linkDir(absolutePath, dest)
        }
        const rel = path.relative(path.dirname(dest), absolutePath)
        return fs.symlink(rel, dest)
      })
  )
}

function pkgShouldBeCopied (pkgData: Package) {
  return pkgData.scripts && (
    pkgData.scripts['install'] ||
    pkgData.scripts['preinstall'] ||
    pkgData.scripts['postinstall'])
}
