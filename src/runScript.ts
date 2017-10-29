import byline = require('byline')
import spawn = require('cross-spawn')
import path = require('path')
import PATH = require('path-name')
import logger from 'pnpm-logger'

const scriptLogger = logger('run_script')

function noop () {} // tslint:disable-line

export default function runScript (
  command: string,
  args: string[],
  opts: {
    cwd: string,
    log: (...msg: string[]) => void,
    userAgent: string,
  },
) {
  opts = Object.assign({log: noop}, opts)
  args = args || []
  const log = opts.log
  const script = `${command}${args.length ? ' ' + args.join(' ') : ''}`
  if (script) scriptLogger.debug('runscript', script)
  if (!command) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const proc = spawn(command, args, {
      cwd: opts.cwd,
      env: createEnv(opts),
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

export function sync (
  command: string,
  args: string[],
  opts: {
    cwd: string,
    stdio: string,
    userAgent?: string,
  },
) {
  opts = Object.assign({}, opts)
  return spawn.sync(command, args, Object.assign({}, opts, {
    env: createEnv(opts),
  }))
}

function createEnv (
  opts: {
    cwd: string,
    userAgent?: string,
  },
) {
  const env = Object.create(process.env)

  env[PATH] = [
    path.join(opts.cwd, 'node_modules', '.bin'),
    path.dirname(process.execPath),
    process.env[PATH],
  ].join(path.delimiter)

  if (opts.userAgent) {
    env.npm_config_user_agent = opts.userAgent
  }

  return env
}
