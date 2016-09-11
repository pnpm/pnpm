import path = require('path')
import createDebug from '../debug'
const debug = createDebug('pnpm:post_install')
import fs = require('mz/fs')
import runScript from '../run_script'
import requireJson from '../fs/require_json'

export default function postInstall (root_: string, log: Function) {
  const root = path.join(root_, '_')
  const pkg = requireJson(path.join(root, 'package.json'))
  debug('postinstall', pkg.name + '@' + pkg.version)
  const scripts = pkg && pkg.scripts || {}
  return Promise.resolve()
    .then(_ => !scripts['install'] && checkBindingGyp(root, log))
    .then(_ => {
      if (scripts['install']) {
        return npmRunScript('install')
      }
      return npmRunScript('preinstall')
        .then(_ => npmRunScript('postinstall'))
    })

  function npmRunScript (scriptName: string) {
    if (!scripts[scriptName]) return Promise.resolve()
    return runScript('npm', ['run', scriptName], { cwd: root, log })
  }
}

/*
 * Run node-gyp when binding.gyp is available. Only do this when there's no
 * `install` script (see `npm help scripts`).
 */

function checkBindingGyp (root: string, log: Function) {
  return fs.stat(path.join(root, 'binding.gyp'))
  .then(() => runScript('node-gyp', ['rebuild'], { cwd: root, log }))
  .catch((err: NodeJS.ErrnoException) => {
    if (err.code !== 'ENOENT') throw err
  })
}
