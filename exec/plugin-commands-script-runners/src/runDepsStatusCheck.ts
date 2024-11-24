import { PnpmError } from '@pnpm/error'
import { install } from '@pnpm/plugin-commands-installation'
import { checkDepsStatus } from '@pnpm/deps.status'
import { prompt } from 'enquirer'
import { type RunOpts } from './run'

export async function runDepsStatusCheck (opts: RunOpts): Promise<void> {
  const { upToDate, issue } = await checkDepsStatus(opts as any) // eslint-disable-line
  if (!upToDate) {
    switch (opts.verifyDepsBeforeRun) {
    case 'install':
      await install.handler(opts as unknown as install.InstallCommandOptions)
      break
    case 'prompt': {
      const confirmed = await prompt<{ runInstall: boolean }>({
        type: 'confirm',
        name: 'runInstall',
        message: `Your "node_modules" directory is out of sync with the "pnpm-lock.yaml" file. This can lead to issues during scripts execution.

  Would you like to run "pnpm install" to update your "node_modules"?`,
        initial: true,
      })
      if (confirmed.runInstall) {
        await install.handler(opts as unknown as install.InstallCommandOptions)
      }
      break
    }
    case 'error':
      throw new PnpmError('VERIFY_DEPS_BEFORE_RUN', issue ?? 'Your node_modules are out of sync with your lockfile', {
        hint: 'Run "pnpm install"',
      })
    }
  }
}
