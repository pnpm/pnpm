'use strict'
const debug = require('debug')('pnpm:run_script')
const path = require('path')
const byline = require('byline')
const spawn = require('cross-spawn')

module.exports = function runScript (command, args, opts) {
  opts = opts || {}
  args = args || []
  const log = opts.log || (() => {})
  const script = `${command}${args.length ? args.join(' ') : ''}`
  if (script) debug('runscript', script)
  if (!command) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const proc = spawn(command, args, {
      cwd: opts.cwd,
      env: createEnv(opts.cwd)
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

module.exports.sync = function (command, args, opts) {
  opts = opts || {}
  return spawn.sync(command, args, Object.assign({}, opts, {
    env: createEnv(opts.cwd)
  }))
}

function createEnv (cwd) {
  const env = Object.create(process.env)
  env.PATH = [
    path.join(cwd, 'node_modules', '.bin'),
    path.dirname(require.resolve('../bin/node-gyp-bin/node-gyp')),
    path.dirname(process.execPath),
    process.env.PATH
  ].join(path.delimiter)
  return env
}
