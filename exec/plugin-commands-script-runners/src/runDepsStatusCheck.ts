import path from 'path'
import { sync as execSync } from 'execa'
import { type VerifyDepsBeforeRun } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { checkDepsStatus, type CheckDepsStatusOptions, type WorkspaceStateSettings } from '@pnpm/deps.status'
import { prompt } from 'enquirer'

export type RunDepsStatusCheckOptions = CheckDepsStatusOptions & { dir: string, verifyDepsBeforeRun?: VerifyDepsBeforeRun }

export async function runDepsStatusCheck (opts: RunDepsStatusCheckOptions): Promise<void> {
  // the following flags are always the default values during `pnpm run` and `pnpm exec`,
  // so they may not match the workspace state after `pnpm install --production|--no-optional`
  const ignoredWorkspaceStateSettings = ['dev', 'optional', 'production'] satisfies Array<keyof WorkspaceStateSettings>
  opts.ignoredWorkspaceStateSettings = ignoredWorkspaceStateSettings

  const { upToDate, issue, workspaceState } = await checkDepsStatus(opts)
  if (upToDate) return
  switch (opts.verifyDepsBeforeRun) {
  case 'install':
    runCommand(createInstallCommand())
    break
  case 'prompt': {
    const installCommand = createInstallCommand()
    const confirmed = await prompt<{ runInstall: boolean }>({
      type: 'confirm',
      name: 'runInstall',
      message: `Your "node_modules" directory is out of sync with the "pnpm-lock.yaml" file. This can lead to issues during scripts execution.

Would you like to run "pnpm ${installCommand.join(' ')}" to update your "node_modules"?`,
      initial: true,
    })
    if (confirmed.runInstall) {
      runCommand(installCommand)
    }
    break
  }
  case 'error':
    throw new PnpmError('VERIFY_DEPS_BEFORE_RUN', issue ?? 'Your node_modules are out of sync with your lockfile', {
      hint: 'Run "pnpm install"',
    })
  case 'warn':
    globalWarn(`Your node_modules are out of sync with your lockfile. ${issue}`)
    break
  }

  type InstallOptions = Array<`--${'no-' | ''}${typeof ignoredWorkspaceStateSettings[number]}`>
  type InstallCommand = ['install', ...InstallOptions]

  function createInstallCommand (): InstallCommand {
    const command: InstallCommand = ['install']
    for (const settingName of ignoredWorkspaceStateSettings) {
      const value: boolean | undefined = workspaceState?.settings[settingName]
      command.push(value ? `--${settingName}` : `--no-${settingName}`)
    }
    return command
  }

  function runCommand (command: InstallCommand): void {
    const execOpts = {
      cwd: opts.dir,
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
}
