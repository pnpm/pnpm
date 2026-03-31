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

  return (execResult.stdout?.toString() ?? '').trim()
}
