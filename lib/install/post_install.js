'use strict'
const join = require('path').join
const debug = require('../debug')('pnpm:post_install')
const fs = require('mz/fs')
const runScript = require('../run_script')

module.exports = function postInstall (root_, pkg, log) {
  const root = join(root_, '_')
  debug('postinstall', pkg.name + '@' + pkg.version)
  const scripts = pkg && pkg.scripts || {}
  return Promise.resolve()
    .then(_ => !scripts.install && checkBindingGyp(root, log))
    .then(_ => {
      if (scripts.install) {
        return npmRunScript('install')
      }
      return npmRunScript('preinstall')
        .then(_ => npmRunScript('postinstall'))
    })

  function npmRunScript (scriptName) {
    if (!scripts[scriptName]) return Promise.resolve()
    return runScript('npm', ['run', scriptName], { cwd: root, log })
  }
}

/*
 * Run node-gyp when binding.gyp is available. Only do this when there's no
 * `install` script (see `npm help scripts`).
 */

function checkBindingGyp (root, log) {
  return fs.stat(join(root, 'binding.gyp'))
  .then(_ => runScript('node-gyp', ['rebuild'], { cwd: root, log }))
  .catch(err => {
    if (err.code !== 'ENOENT') throw err
  })
}
