import logger from 'pnpm-logger'
import path = require('path')
import byline = require('byline')
import spawn = require('cross-spawn')
import PATH = require('path-name')

const scriptLogger = logger('run_script')

export type RunScriptOptions = {
  cwd: string,
  log: Function
}

export default function runScript (command: string, args: string[], opts: RunScriptOptions) {
  opts = Object.assign({log: (() => {})}, opts)
  args = args || []
  const log = opts.log
  const script = `${command}${args.length ? ' ' + args.join(' ') : ''}`
  if (script) scriptLogger.debug('runscript', script)
  if (!command) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const proc = spawn(command, args, {
      cwd: opts.cwd,
      env: createEnv(opts.cwd)
    })

    log('stdout', '$ ' + script)

    proc.on('error', reject)
    byline(proc.stdout).on('data', (line: Buffer) => log('stdout', line.toString()))
    byline(proc.stderr).on('data', (line: Buffer) => log('stderr', line.toString()))

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      return resolve()
    })
  })
}

export type RunSyncScriptOptions = {
  cwd: string,
  stdio: string
}

export function sync (command: string, args: string[], opts: RunSyncScriptOptions) {
  opts = Object.assign({}, opts)
  return spawn.sync(command, args, Object.assign({}, opts, {
    env: createEnv(opts.cwd)
  }))
}

function createEnv (cwd: string) {
  const env = Object.create(process.env)
  env[PATH] = [
    path.join(cwd, 'node_modules', '.bin'),
    path.dirname(process.execPath),
    process.env[PATH]
  ].join(path.delimiter)
  env['NODE_PRESERVE_SYMLINKS'] = '1'
  return env
}
