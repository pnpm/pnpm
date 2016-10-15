import path = require('path')
import requireJson from '../fs/requireJson'
import relSymlink from '../fs/relSymlink'
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
  const pkgData = requireJson(path.join(target, 'package.json'))
  if (!pkgData.name) { throw new Error('Invalid package.json for ' + target) }

  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  const out = path.join(modules, pkgData.name)
  await mkdirp(path.dirname(out))
  await relSymlink(target, out)
}
