var Promise = require('../promise')
var join = require('path').join
var spawn = require('child_process').spawn
var debug = require('debug')('pnpm:post_install')
var delimiter = require('path').delimiter
var byline = require('byline')

module.exports = function postInstall (root, package, log) {
  debug('postinstall', package)
  var scripts = package && package.scripts || {}
  return Promise.resolve()
    .then(_ => runScript(root, scripts.preinstall, log))
    .then(_ => runScript(root, scripts.install, log))
    .then(_ => runScript(root, scripts.postinstall, log))
}

/*
 * Runs an npm script.
 */

function runScript (root, script, log) {
  debug('runscript', script)
  if (!script) return Promise.resolve()
  return new Promise((resolve, reject) => {
    var env = Object.create(process.env)
    env.PATH = [join(root, 'node_modules', '.bin'), process.env.PATH].join(delimiter)

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
