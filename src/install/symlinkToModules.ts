import path = require('path')
import fs = require('mz/fs')
import requireJson from '../fs/requireJson'
import linkDir from 'link-dir'
import mkdirp from '../fs/mkdirp'

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

  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  const out = path.join(modules, pkgData.name)
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
