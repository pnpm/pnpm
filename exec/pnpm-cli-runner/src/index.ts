import path from 'node:path'

import { sync as execSync } from 'execa'

export interface RunPnpmCliOptions {
  cwd: string
  silent?: boolean
}

export function runPnpmCli (command: string[], { cwd, silent }: RunPnpmCliOptions): void {
  const execOpts = {
    cwd,
    stdio: silent ? ['inherit', 'ignore', 'inherit'] as const : 'inherit' as const,
  }
  const cliCommand = silent ? [...command, '--reporter=silent'] : command
  const execFileName = path.basename(process.execPath).toLowerCase()
  if (execFileName === 'pnpm' || execFileName === 'pnpm.exe') {
    execSync(process.execPath, cliCommand, execOpts)
  } else if (path.basename(process.argv[1]) === 'pnpm.mjs') {
    execSync(process.execPath, [process.argv[1], ...cliCommand], execOpts)
  } else {
    execSync('pnpm', cliCommand, execOpts)
  }
}
