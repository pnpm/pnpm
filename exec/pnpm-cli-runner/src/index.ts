import path from 'node:path'

import { sync as execSync } from 'execa'

export interface RunPnpmCliOptions {
  cwd: string
  reporter?: string
}

export function runPnpmCli (command: string[], { cwd, reporter }: RunPnpmCliOptions): void {
  const execOpts = {
    cwd,
    stdio: 'inherit' as const,
  }
  const cliCommand = reporter ? [...command, `--reporter=${reporter}`] : command
  const execFileName = path.basename(process.execPath).toLowerCase()
  if (execFileName === 'pnpm' || execFileName === 'pnpm.exe') {
    execSync(process.execPath, cliCommand, execOpts)
  } else if (path.basename(process.argv[1]) === 'pnpm.mjs') {
    execSync(process.execPath, [process.argv[1], ...cliCommand], execOpts)
  } else {
    execSync('pnpm', cliCommand, execOpts)
  }
}
