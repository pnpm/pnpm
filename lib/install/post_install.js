var Promise = require('../promise')
var join = require('path').join
var dirname = require('path').dirname
var spawn = require('child_process').spawn
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
        .then(_ => npmRunScript(scripts.postinstall))
    })

  function npmRunScript (scriptName) {
    if (!scriptName || !scripts[scriptName]) return Promise.resolve()
    return runScript(root, 'npm run ' + scriptName, log)
  }
}

/*
 * Run node-gyp when binding.gyp is available. Only do this when there's no
 * `install` script (see `npm help scripts`).
 */

function checkBindingGyp (root, log) {
  return fs.stat(join(root, 'binding.gyp'))
  .then(_ => runScript(root, 'node-gyp rebuild', log))
  .catch(err => {
    if (err.code !== 'ENOENT') throw err
  })
}

/*
 * Runs an npm script.
 */

function runScript (root, script, log) {
  if (script) debug('runscript', script)
  if (!script) return Promise.resolve()
  return new Promise((resolve, reject) => {
    var env = Object.create(process.env)
    env.PATH = [
      join(root, 'node_modules', '.bin'),
      dirname(require.resolve('../../bin/node-gyp-bin/node-gyp')),
      process.env.PATH
    ].join(delimiter)

    var proc = spawn('sh', ['-c', script], {
      cwd: root,
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
