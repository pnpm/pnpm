import path from 'path'
import { sync as execSync } from 'execa'

export function runPnpmCli (
  command: string[],
  { cwd, env }: { cwd: string, env?: NodeJS.ProcessEnv }
): void {
  const execOpts = {
    cwd,
    stdio: 'inherit' as const,
    ...(env ? { env } : {}),
  }
  const execFileName = path.basename(process.execPath).toLowerCase()
  if (execFileName === 'pnpm' || execFileName === 'pnpm.exe') {
    execSync(process.execPath, command, execOpts)
  } else if (path.basename(process.argv[1]) === 'pnpm.cjs') {
    execSync(process.execPath, [process.argv[1], ...command], execOpts)
  } else {
    execSync('pnpm', command, execOpts)
  }
}
