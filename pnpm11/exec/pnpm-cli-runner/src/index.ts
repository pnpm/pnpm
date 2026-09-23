import { resolvePnpmSelfCommand } from '@pnpm/cli.meta'
import { sync as execSync } from 'execa'

export interface RunPnpmCliOptions {
  cwd: string
  reporter?: string
}

export function runPnpmCli (command: string[], { cwd, reporter }: RunPnpmCliOptions): void {
  const cliCommand = reporter ? [...command, `--reporter=${reporter}`] : command
  const [executable, ...selfArgs] = resolvePnpmSelfCommand()
  execSync(executable, [...selfArgs, ...cliCommand], {
    cwd,
    stdio: 'inherit',
  })
}
