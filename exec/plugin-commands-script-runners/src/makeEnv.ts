import { PnpmError } from '@pnpm/error'
import path from 'path'
import PATH from 'path-name'

export function makeEnv (
  opts: {
    extraEnv?: NodeJS.ProcessEnv
    userAgent?: string
    prependPaths: string[]
  }
) {
  for (const prependPath of opts.prependPaths) {
    if (prependPath.includes(path.delimiter)) {
      // Unfortunately, there is no way to escape the PATH delimiter,
      // so directories added to the PATH should not contain it.
      throw new PnpmError('BAD_PATH_DIR', `Cannot add ${prependPath} to PATH because it contains the path delimiter character (${path.delimiter})`)
    }
  }
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
