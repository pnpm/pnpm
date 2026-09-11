import path from 'node:path'

import { sync as execSync } from 'execa'

export interface RunPnpmCliOptions {
  cwd: string
  reporter?: string
}

function isPnpmScript (scriptPath: string): boolean {
  const scriptName = path.basename(scriptPath).toLowerCase()
  return scriptName === 'pnpm' || scriptName.startsWith('pnpm.') || scriptName.startsWith('pnpm-')
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
  } else if (process.argv[1] && isPnpmScript(process.argv[1])) {
    execSync(process.execPath, [process.argv[1], ...cliCommand], execOpts)
  } else {
    execSync('pnpm', cliCommand, execOpts)
  }
}
