'use strict'
var join = require('path').join
var dirname = require('path').dirname
var spawn = require('cross-spawn')
var debug = require('debug')('pnpm:post_install')
var delimiter = require('path').delimiter
var byline = require('byline')
var fs = require('mz/fs')

module.exports = function postInstall (root_, pkg, log) {
  var root = join(root_, '_')
  debug('postinstall', pkg.name + '@' + pkg.version)
  var scripts = pkg && pkg.scripts || {}
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

/*
 * Runs an npm script.
 */

function runScript (command, args, opts) {
  opts = opts || {}
  args = args || []
  const log = opts.log || (() => {})
  const script = `${command}${args.length ? args.join(' ') : ''}`
  if (script) debug('runscript', script)
  if (!command) return Promise.resolve()
  return new Promise((resolve, reject) => {
    var env = Object.create(process.env)
    env.PATH = [
      join(opts.cwd, 'node_modules', '.bin'),
      dirname(require.resolve('../../bin/node-gyp-bin/node-gyp')),
      process.env.PATH
    ].join(delimiter)

    var proc = spawn(command, args, {
      cwd: opts.cwd,
      env: env
    })

    log('stderr', '$ ' + script)

    proc.on('error', reject)
    byline(proc.stdout).on('data', line => log('stdout', line))
    byline(proc.stderr).on('data', line => log('stderr', line))

    proc.on('close', code => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      return resolve()
    })
  })
}
