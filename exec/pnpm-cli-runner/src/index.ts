import path from 'path'
import { sync as execSync } from 'execa'

export function runPnpmCli (command: string[], { cwd }: { cwd: string }): void {
  const execOpts = {
    cwd,
    stdio: 'inherit' as const,
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
