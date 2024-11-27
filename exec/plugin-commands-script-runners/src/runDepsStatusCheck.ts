import path from 'path'
import { sync as execSync } from 'execa'
import { type VerifyDepsBeforeRun } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { checkDepsStatus, type CheckDepsStatusOptions } from '@pnpm/deps.status'
import { prompt } from 'enquirer'

export type RunDepsStatusCheckOptions = CheckDepsStatusOptions & { dir: string, verifyDepsBeforeRun?: VerifyDepsBeforeRun }

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
    const execOpts = {
      cwd: opts.dir,
      stdio: 'inherit' as const,
    }
    if (path.basename(process.execPath) === 'pnpm') {
      execSync(process.execPath, ['install'], execOpts)
    } else if (path.basename(process.argv[1]) === 'pnpm.cjs') {
      execSync(process.execPath, [process.argv[1], 'install'], execOpts)
    } else {
      execSync('pnpm', ['install'], execOpts)
    }
  }
}
