import { sync as execSync } from 'execa'
import { type VerifyDepsBeforeRun } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { checkDepsStatus, type CheckDepsStatusOptions } from '@pnpm/deps.status'
import { prompt } from 'enquirer'

export type RunDepsStatusCheckOptions = CheckDepsStatusOptions & { verifyDepsBeforeRun?: VerifyDepsBeforeRun }

export async function runDepsStatusCheck (opts: RunDepsStatusCheckOptions): Promise<void> {
  const { upToDate, issue } = await checkDepsStatus(opts)
  if (upToDate) return
  switch (opts.verifyDepsBeforeRun) {
  case 'install':
    install()
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

  function install () {
    execSync('pnpm', ['install'], {
      cwd: opts.dir,
      stdio: 'inherit',
    })
  }
}
