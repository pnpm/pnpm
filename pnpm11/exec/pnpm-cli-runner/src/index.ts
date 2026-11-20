import { resolvePnpmSelfCommand } from '@pnpm/cli.meta'
import { sync as execSync } from 'execa'

export interface RunPnpmCliOptions {
  cwd: string
  loglevel?: string
  reporter?: string
}

export function runPnpmCli (command: string[], { cwd, loglevel, reporter }: RunPnpmCliOptions): void {
  const cliCommand = [...command]
  if (reporter) cliCommand.push(`--reporter=${reporter}`)
  if (loglevel) cliCommand.push(`--loglevel=${loglevel}`)
  const [executable, ...selfArgs] = resolvePnpmSelfCommand()
  execSync(executable, [...selfArgs, ...cliCommand], {
    cwd,
    stdio: 'inherit',
  })
}
