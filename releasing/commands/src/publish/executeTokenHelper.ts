import { sync as execa } from 'execa'

export interface ExecuteTokenHelperOptions {
  globalWarn: (message: string) => void
}

export function executeTokenHelper ([cmd, ...args]: [string, ...string[]], opts: ExecuteTokenHelperOptions): string {
  const execResult = execa(cmd, args, {
    stdio: 'pipe',
  })

  const stderr = execResult.stderr?.toString() ?? ''
  if (stderr.trim()) {
    for (const line of stderr.trimEnd().split('\n')) {
      opts.globalWarn(`(tokenHelper stderr) ${line}`)
    }
  }

  const token = (execResult.stdout?.toString() ?? '').trim()
  // If the token helper output includes an auth scheme prefix (e.g. "Bearer ..."),
  // strip it since libnpmpublish adds the scheme itself.
  return token.replace(/^Bearer\s+/i, '')
}
