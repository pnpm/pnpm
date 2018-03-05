import logger from '@pnpm/logger'
import byline = require('byline')
import spawn = require('cross-spawn')
import path = require('path')
import PATH = require('path-name')
import {lifecycleLogger} from './loggers'

const scriptLogger = logger('run_script')

export default function runScript (
  command: string,
  args: string[],
  opts: {
    cwd: string,
    pkgId: string,
    userAgent: string,
  },
) {
  opts = Object.assign({log: (() => {})}, opts) // tslint:disable-line:no-empty
  args = args || []
  const script = `${command}${args.length ? ' ' + args.join(' ') : ''}`
  if (script) scriptLogger.debug(`runscript ${script}`)
  if (!command) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const proc = spawn(command, args, {
      cwd: opts.cwd,
      env: createEnv(opts),
    })

    const scriptName = args[args.length - 1]

    proc.on('error', reject)
    byline(proc.stdout).on('data', (line: Buffer) => lifecycleLogger.info({
      line: line.toString(),
      pkgId: opts.pkgId,
      script: scriptName,
    }))
    byline(proc.stderr).on('data', (line: Buffer) => lifecycleLogger.error({
      line: line.toString(),
      pkgId: opts.pkgId,
      script: scriptName,
    }))

    proc.on('close', (code: number) => {
      if (code > 0) {
        lifecycleLogger.error({
          exitCode: code,
          pkgId: opts.pkgId,
          script: scriptName,
        })
        return reject(new Error('Exit code ' + code))
      }
      lifecycleLogger.info({
        exitCode: code,
        pkgId: opts.pkgId,
        script: scriptName,
      })
      return resolve()
    })
  })
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
    env['npm_config_user_agent'] = opts.userAgent // tslint:disable-line:no-string-literal
  }

  return env
}
