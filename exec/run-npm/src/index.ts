import type childProcess from 'child_process'
import path from 'path'
import spawn from 'cross-spawn'
import PATH from 'path-name'

export type NPMLocation = 'global' | 'user' | 'project'

export interface RunNPMOptions {
  cwd?: string
  env?: Record<string, string>
  location?: NPMLocation
  userConfigPath?: string
}

export function runNpm (npmPath: string | undefined, args: string[], options?: RunNPMOptions): childProcess.SpawnSyncReturns<Buffer> {
  const npm = npmPath ?? 'npm'
  return runScriptSync(npm, args, {
    cwd: options?.cwd ?? process.cwd(),
    stdio: 'inherit',
    userAgent: undefined,
    env: { ...options?.env, COREPACK_ENABLE_STRICT: '0' },
    location: options?.location,
    userConfigPath: options?.userConfigPath,
  })
}

export function runScriptSync (
  command: string,
  args: string[],
  opts: {
    cwd: string
    location?: NPMLocation
    stdio: childProcess.StdioOptions
    userAgent?: string
    userConfigPath?: string
    env: Record<string, string>
  }
): childProcess.SpawnSyncReturns<Buffer> {
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
    location?: NPMLocation
    userAgent?: string
    userConfigPath?: string
  }
): NodeJS.ProcessEnv {
  const env = { ...process.env }

  env[PATH] = [
    path.join(opts.cwd, 'node_modules', '.bin'),
    path.dirname(process.execPath),
    process.env[PATH],
  ].join(path.delimiter)

  if (opts.userAgent) {
    env.npm_config_user_agent = opts.userAgent
  }

  if (opts.location) {
    env.npm_config_location = opts.location
  }

  if (opts.userConfigPath) {
    env.npm_config_userconfig = opts.userConfigPath
  }

  return env
}
