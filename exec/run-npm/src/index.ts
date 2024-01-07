import type childProcess from 'child_process'
import path from 'path'
import spawn from 'cross-spawn'
import PATH from 'path-name'

export interface RunNPMOptions {
  cwd?: string
  env?: Record<string, string>
}

export function runNpm (npmPath: string | undefined, args: string[], options?: RunNPMOptions) {
  const npm = npmPath ?? 'npm'
  return runScriptSync(npm, args, {
    cwd: options?.cwd ?? process.cwd(),
    stdio: 'inherit',
    userAgent: undefined,
    env: options?.env ?? {},
  })
}

export function runScriptSync (
  command: string,
  args: string[],
  opts: {
    cwd: string
    stdio: childProcess.StdioOptions
    userAgent?: string
    env: Record<string, string>
  }
) {
  const env = {
    ...createEnv(opts),
    ...opts.env,
  }
  const result = spawn.sync(command, args, {
    ...opts,
    env,
  })
  if (result.error) throw result.error
  return result
}

function createEnv (
  opts: {
    cwd: string
    userAgent?: string
  }
) {
  const env = { ...process.env }

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
