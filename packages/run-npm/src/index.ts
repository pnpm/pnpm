import childProcess = require('child_process')
import path = require('path')
import spawn = require('cross-spawn')
import PATH = require('path-name')

export default function runNpm (npmPath: string | undefined, args: string[]) {
  const npm = npmPath ?? 'npm'
  return runScriptSync(npm, args, {
    cwd: process.cwd(),
    stdio: 'inherit',
    userAgent: undefined,
  })
}

export function runScriptSync (
  command: string,
  args: string[],
  opts: {
    cwd: string
    stdio: childProcess.StdioOptions
    userAgent?: string
  }
) {
  opts = Object.assign({}, opts)
  return spawn.sync(command, args, Object.assign({}, opts, {
    env: createEnv(opts),
  }))
}

function createEnv (
  opts: {
    cwd: string
    userAgent?: string
  }
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
