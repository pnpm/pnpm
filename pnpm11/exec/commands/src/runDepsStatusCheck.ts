import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { confirm } from '@inquirer/prompts'
import type { Config, VerifyDepsBeforeRun } from '@pnpm/config.reader'
import { createHexHash } from '@pnpm/crypto.hash'
import { checkDepsStatus, type CheckDepsStatusOptions, type WorkspaceStateSettings } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { globalWarn } from '@pnpm/logger'
import { realpathMissing } from 'realpath-missing'

import { DirLock } from './dirLock.js'

const INSTALL_LOCK_NAMESPACE = 'pnpm-verify-deps-install-locks'
// How long a gate waits for another gate's install in the same workspace
// before installing without the lock.
const INSTALL_LOCK_WAIT_MS = 5 * 60_000
// Comfortably above how long an install can legitimately take.
const INSTALL_LOCK_ABANDONED_MS = 30 * 60_000

export interface RunDepsStatusCheckOptions extends CheckDepsStatusOptions {
  dir: string
  reporter?: Config['reporter']
  verifyDepsBeforeRun?: VerifyDepsBeforeRun
}

export async function runDepsStatusCheck (opts: RunDepsStatusCheckOptions): Promise<void> {
  // the following flags are always the default values during `pnpm run` and `pnpm exec`,
  // so they may not match the workspace state after `pnpm install --prod|--no-optional`
  const ignoredWorkspaceStateSettings = ['dev', 'optional', 'production'] satisfies Array<keyof WorkspaceStateSettings>
  opts.ignoredWorkspaceStateSettings = ignoredWorkspaceStateSettings

  const { upToDate, issue, workspaceState } = await checkDepsStatus(opts)
  if (!needsInstall(upToDate, opts)) return

  const command = ['install', ...createInstallArgs(workspaceState?.settings)]
  const install = lockedInstall.bind(null, opts, command)

  switch (opts.verifyDepsBeforeRun) {
    case 'install':
      await install()
      break
    case 'prompt': {
    // In non-TTY environments (like CI), we can't prompt the user
    // Exit with error to alert users that node_modules are out of sync
      if (!process.stdin.isTTY) {
        throw new PnpmError('VERIFY_DEPS_BEFORE_RUN', issue ?? 'Your node_modules are out of sync with your lockfile', {
          hint: 'Run "pnpm install" before running scripts. The "verifyDepsBeforeRun: prompt" setting cannot prompt for confirmation in non-interactive environments.',
        })
      }
      let confirmed: boolean
      try {
        confirmed = await confirm({
          message: `Your "node_modules" directory is out of sync with the "pnpm-lock.yaml" file. This can lead to issues during scripts execution.

Would you like to run "pnpm ${command.join(' ')}" to update your "node_modules"?`,
          default: true,
        })
      } catch (err: unknown) {
        if (err instanceof Error && err.name === 'ExitPromptError') {
          process.exit(1)
        }
        throw err
      }
      if (confirmed) {
        await install()
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

function needsInstall (upToDate: boolean | undefined, opts: RunDepsStatusCheckOptions): boolean {
  if (upToDate === true) return false
  return upToDate === false || opts.allProjects != null || opts.rootProjectManifest != null
}

/**
 * Installs while holding the workspace's gate lock, so concurrent `run` and
 * `exec` gates on one stale tree start one install rather than one each,
 * racing in the same `node_modules`. A gate that found the lock held
 * re-checks the dependencies once it gets the lock, and installs only if its
 * predecessor's install left them out of date.
 */
async function lockedInstall (opts: RunDepsStatusCheckOptions, command: string[]): Promise<void> {
  const root = opts.workspaceDir ?? opts.dir
  let lock: DirLock | undefined
  let waited = false
  try {
    const lockPath = await installLockPath(root)
    lock = await DirLock.acquire(lockPath, { waitMs: 0, abandonedMs: INSTALL_LOCK_ABANDONED_MS })
    if (lock == null) {
      waited = true
      lock = await DirLock.acquire(lockPath, { waitMs: INSTALL_LOCK_WAIT_MS, abandonedMs: INSTALL_LOCK_ABANDONED_MS })
    }
  } catch (err: unknown) {
    const message = util.types.isNativeError(err) ? err.message : String(err)
    globalWarn(`Could not lock the dependency install at ${root}: ${message}. Installing without it, which is unsafe if another pnpm is installing there concurrently.`)
  }
  try {
    if (waited) {
      const { upToDate, workspaceState } = await checkDepsStatus(opts)
      if (!needsInstall(upToDate, opts)) return
      command = ['install', ...createInstallArgs(workspaceState?.settings)]
    }
    runPnpmCli(command, { cwd: opts.dir, reporter: opts.reporter })
  } finally {
    await lock?.release()
  }
}

async function installLockPath (root: string): Promise<string> {
  const uid = process.getuid?.()
  const lockDir = path.join(os.tmpdir(), uid == null ? INSTALL_LOCK_NAMESPACE : `${INSTALL_LOCK_NAMESPACE}-${uid}`)
  await fs.mkdir(lockDir, { recursive: true, mode: 0o700 })
  const stats = await fs.lstat(lockDir)
  if (!stats.isDirectory() || (uid != null && (stats.uid !== uid || (stats.mode & 0o077) !== 0))) {
    throw new Error(`${lockDir} is not a private directory of the current user`)
  }
  return path.join(lockDir, `${createHexHash(await realpathMissing(root))}.lock`)
}

export function createInstallArgs (opts: Pick<WorkspaceStateSettings, 'dev' | 'optional' | 'production'> | undefined): string[] {
  const args: string[] = []
  if (!opts) return args
  const { dev, optional, production } = opts
  if (production && !dev) {
    args.push('--prod')
  } else if (dev && !production) {
    args.push('--dev')
  }
  if (!optional) {
    args.push('--no-optional')
  }
  return args
}
