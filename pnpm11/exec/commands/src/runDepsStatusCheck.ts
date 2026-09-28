import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { confirm } from '@inquirer/prompts'
import { type Config, getIgnoredLockfilePnpmFieldKeys, type VerifyDepsBeforeRun } from '@pnpm/config.reader'
import { createHexHash } from '@pnpm/crypto.hash'
import { checkDepsStatus, type CheckDepsStatusOptions, type CheckDepsStatusResult, type WorkspaceStateSettings } from '@pnpm/deps.status'
import { isError, PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { DirLock } from '@pnpm/fs.dir-lock'
import { globalWarn } from '@pnpm/logger'
import type { ProjectManifest } from '@pnpm/types'
import { leftOutOfEnclosingWorkspace } from '@pnpm/workspace.root-finder'
import { realpathMissing } from 'realpath-missing'

const INSTALL_LOCK_NAMESPACE = 'pnpm-verify-deps-install-locks'
// How long a gate waits for another gate's install in the same workspace
// before installing without the lock.
const INSTALL_LOCK_WAIT_MS = 5 * 60_000
// Comfortably above how long an install can legitimately take.
const INSTALL_LOCK_ABANDONED_MS = 30 * 60_000

export interface RunDepsStatusCheckOptions extends CheckDepsStatusOptions, Partial<Pick<Config, 'filter' | 'filterProd'>> {
  dir: string
  loglevel?: Config['loglevel']
  reporter?: Config['reporter']
  verifyDepsBeforeRun?: VerifyDepsBeforeRun
}

export async function runDepsStatusCheck (opts: RunDepsStatusCheckOptions): Promise<void> {
  // the following flags are always the default values during `pnpm run` and `pnpm exec`,
  // so they may not match the workspace state after `pnpm install --prod|--no-optional`
  const ignoredWorkspaceStateSettings = ['dev', 'optional', 'production'] satisfies Array<keyof WorkspaceStateSettings>
  opts.ignoredWorkspaceStateSettings = ignoredWorkspaceStateSettings

  const { upToDate, issue, workspaceState } = await checkDepsStatus(opts)
  if (await installNotRequired(opts, upToDate, workspaceState)) return

  const command = ['install', ...createInstallArgs(workspaceState?.settings), ...createFilterArgs(opts)]
  const install = lockedInstall.bind(null, opts, command)

  switch (opts.verifyDepsBeforeRun) {
    case 'install':
      await install()
      break
    case 'prompt': {
    // In non-TTY environments (like CI), we can't prompt the user
    // Exit with error to alert users that node_modules are out of sync
      if (!process.stdin.isTTY) {
        refuseInstallDroppingIgnoredSettings(opts)
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
        if (isError(err) && err.name === 'ExitPromptError') {
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

/**
 * An install would ignore the settings the root manifest still keeps in its
 * `pnpm` field and rewrite the lockfile without the ones the lockfile records.
 * That drops them silently, so the gate leaves the decision to an explicit
 * `pnpm install` after the settings have moved.
 */
function refuseInstallDroppingIgnoredSettings (opts: RunDepsStatusCheckOptions): void {
  if (opts.rootProjectManifest == null) return
  const keys = getIgnoredLockfilePnpmFieldKeys(opts.rootProjectManifest)
  if (keys.length === 0) return
  const quotedKeys = keys.map(key => `"pnpm.${key}"`).join(', ')
  throw new PnpmError('VERIFY_DEPS_BEFORE_RUN', `Your node_modules are out of sync with your lockfile, and installing would drop ${quotedKeys} from the lockfile, because the "pnpm" field in package.json is no longer read by pnpm`, {
    hint: 'Move these settings to pnpm-workspace.yaml (see https://pnpm.io/settings), then run "pnpm install".',
  })
}

function needsInstall (upToDate: boolean | undefined, opts: RunDepsStatusCheckOptions): boolean {
  if (upToDate === true) return false
  return upToDate === false || opts.allProjects != null || opts.rootProjectManifest != null
}

/**
 * A dependency-free project an enclosing workspace leaves out has nothing
 * to install. Spawning one would write a lockfile into a directory the
 * workspace install left alone.
 */
async function installNotRequired (
  opts: RunDepsStatusCheckOptions,
  upToDate: boolean | undefined,
  workspaceState: CheckDepsStatusResult['workspaceState']
): Promise<boolean> {
  if (!needsInstall(upToDate, opts)) return true
  if (workspaceState != null) return false
  return dependencyFreeProjectLeftOut(opts)
}

async function dependencyFreeProjectLeftOut (opts: RunDepsStatusCheckOptions): Promise<boolean> {
  if (opts.workspaceDir != null || hasRuntimeDependencies(opts)) return false
  return leftOutOfEnclosingWorkspace(opts.dir)
}

function hasRuntimeDependencies (opts: RunDepsStatusCheckOptions): boolean {
  if (manifestHasRuntimeDependencies(opts.rootProjectManifest)) return true
  return opts.allProjects?.some(project => manifestHasRuntimeDependencies(project.manifest)) ?? false
}

function manifestHasRuntimeDependencies (manifest: ProjectManifest | undefined): boolean {
  if (manifest == null) return false
  return [manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies]
    .some(group => group != null && Object.keys(group).length > 0)
}

/**
 * Installs while holding the workspace's gate lock, so concurrent `run` and
 * `exec` gates on one stale tree start one install rather than one each,
 * racing in the same `node_modules`. A gate that found the lock held
 * re-checks the dependencies once it gets the lock, and installs only if its
 * predecessor's install left them out of date.
 */
async function lockedInstall (opts: RunDepsStatusCheckOptions, command: string[]): Promise<void> {
  refuseInstallDroppingIgnoredSettings(opts)
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
    const message = isError(err) ? err.message : String(err)
    globalWarn(`Could not lock the dependency install at ${root}: ${message}. Installing without it, which is unsafe if another pnpm is installing there concurrently.`)
  }
  try {
    if (waited) {
      const { upToDate, workspaceState } = await checkDepsStatus(opts)
      if (await installNotRequired(opts, upToDate, workspaceState)) return
      command = ['install', ...createInstallArgs(workspaceState?.settings), ...createFilterArgs(opts)]
    }
    const loglevel = opts.loglevel === 'silent' || opts.loglevel === 'error' || opts.loglevel === 'warn' ? opts.loglevel : undefined
    runPnpmCli(command, { cwd: opts.dir, loglevel, reporter: opts.reporter })
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

/**
 * The install that the gate spawns has to select the same projects the command
 * being gated was filtered to, otherwise a filtered `run` or `exec` would
 * install every project of the workspace. Each selector also selects its
 * dependencies, because a selected project needs the workspace projects it
 * depends on installed too.
 */
export function createFilterArgs (opts: Pick<RunDepsStatusCheckOptions, 'filter' | 'filterProd'>): string[] {
  return [
    ...(opts.filter ?? []).map((selector) => `--filter=${withDependencies(selector)}`),
    ...(opts.filterProd ?? []).map((selector) => `--filter-prod=${withDependencies(selector)}`),
  ]
}

function withDependencies (selector: string): string {
  return selector.startsWith('!') || selector.endsWith('...') ? selector : `${selector}...`
}
