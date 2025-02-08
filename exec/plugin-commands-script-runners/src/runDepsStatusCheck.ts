import { type VerifyDepsBeforeRun } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
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

  const command = ['install', ...createInstallArgs(workspaceState?.settings)]
  const install = runPnpmCli.bind(null, command, { cwd: opts.dir })

  switch (opts.verifyDepsBeforeRun) {
  case 'install':
    install()
    break
  case 'prompt': {
    const confirmed = await prompt<{ runInstall: boolean }>({
      type: 'confirm',
      name: 'runInstall',
      message: `Your "node_modules" directory is out of sync with the "pnpm-lock.yaml" file. This can lead to issues during scripts execution.

Would you like to run "pnpm ${command.join(' ')}" to update your "node_modules"?`,
      initial: true,
    })
    if (confirmed.runInstall) {
      install()
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
}

export function createInstallArgs (opts: Pick<WorkspaceStateSettings, 'dev' | 'optional' | 'production'> | undefined): string[] {
  const args: string[] = []
  if (!opts) return args
  const { dev, optional, production } = opts
  if (production && !dev) {
    args.push('--production')
  } else if (dev && !production) {
    args.push('--dev')
  }
  if (!optional) {
    args.push('--no-optional')
  }
  return args
}
