import { sync as execa } from 'execa'
import { logger } from '@pnpm/logger'

export const stderrLogger = logger('stderr')

export function executeTokenHelper ([cmd, ...args]: [string, ...string[]]): string {
  const execResult = execa(cmd, args, {
    stdio: 'pipe',
  })

  if (execResult.stderr.trim()) {
    const prefix = process.cwd()
    for (const line of execResult.stderr.trimEnd().split('\n')) {
      stderrLogger.warn({
        prefix,
        message: `tokenHelper stderr: ${line}`,
      })
    }
  }

  return execResult.stdout.trim()
}
