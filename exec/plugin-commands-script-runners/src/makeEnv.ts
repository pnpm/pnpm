import path from 'path'
import PATH from 'path-name'

export function makeEnv (
  opts: {
    extraEnv?: NodeJS.ProcessEnv
    userAgent?: string
    prependPaths: string[]
  }
) {
  return {
    ...process.env,
    ...opts.extraEnv,
    npm_config_user_agent: opts.userAgent ?? 'pnpm',
    [PATH]: [
      ...opts.prependPaths,
      process.env[PATH],
    ].join(path.delimiter),
  }
}
