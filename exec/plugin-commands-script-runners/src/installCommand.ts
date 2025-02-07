import path from 'path'
import { sync as execSync } from 'execa'
import { type WorkspaceStateSettings } from '@pnpm/deps.status'

export type InstallOptions = Array<'--production' | '--dev' | '--no-optional'>
export type InstallCommand = ['install', ...InstallOptions]

export function fromWorkspaceStateSettings (settings: WorkspaceStateSettings | undefined): InstallCommand {
  const command: InstallCommand = ['install']
  if (!settings) return command
  const { dev, optional, production } = settings
  if (production && !dev) {
    command.push('--production')
  } else if (dev && !production) {
    command.push('--dev')
  }
  if (!optional) {
    command.push('--no-optional')
  }
  return command
}

export function run (cwd: string, command: InstallCommand): void {
  const execOpts = {
    cwd,
    stdio: 'inherit' as const,
  }
  if (path.basename(process.execPath) === 'pnpm') {
    execSync(process.execPath, command, execOpts)
  } else if (path.basename(process.argv[1]) === 'pnpm.cjs') {
    execSync(process.execPath, [process.argv[1], ...command], execOpts)
  } else {
    execSync('pnpm', command, execOpts)
  }
}
