import { existsSync } from 'node:fs'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { confirm } from '@inquirer/prompts'
import { type Config, getIgnoredLockfilePnpmFieldKeys, type VerifyDepsBeforeRun } from '@pnpm/config.reader'
import { createHexHash } from '@pnpm/crypto.hash'
import { checkDepsStatus, type CheckDepsStatusOptions, type CheckDepsStatusResult, type WorkspaceStateSettings } from '@pnpm/deps.status'
import { isError, PnpmError } from '@pnpm/error'
import { PROJECT_LIFECYCLE_STAGES } from '@pnpm/exec.lifecycle'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { DirLock } from '@pnpm/fs.dir-lock'
import { globalWarn } from '@pnpm/logger'
import type { ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { realpathMissing } from 'realpath-missing'

import { type ProjectsToVerifyOptions, selectProjectsToVerify, withDependencies } from './selectProjectsToVerify.js'

const INSTALL_LOCK_NAMESPACE = 'pnpm-verify-deps-install-locks'
// How long a gate waits for another gate's install in the same workspace
// before installing without the lock.
const INSTALL_LOCK_WAIT_MS = 5 * 60_000
// Comfortably above how long an install can legitimately take.
const INSTALL_LOCK_ABANDONED_MS = 30 * 60_000

export interface RunDepsStatusCheckOptions extends CheckDepsStatusOptions, ProjectsToVerifyOptions, Partial<Pick<Config, 'ignoreScripts' | 'workspacePackagePatterns'>> {
  dir: string
  loglevel?: Config['loglevel']
  reporter?: Config['reporter']
  verifyDepsBeforeRun?: VerifyDepsBeforeRun
  rawCliConfig?: Record<string, unknown>
}

export async function runDepsStatusCheck (commandOpts: RunDepsStatusCheckOptions): Promise<void> {
  // The expanded selection has to reach the recheck `lockedInstall` makes
  // after waiting for another install as well.
  const opts: RunDepsStatusCheckOptions = {
    ...commandOpts,
    // the following flags are always the default values during `pnpm run` and `pnpm exec`,
    // so they may not match the workspace state after `pnpm install --prod|--no-optional`
    ignoredWorkspaceStateSettings: ['dev', 'optional', 'production'] satisfies Array<keyof WorkspaceStateSettings>,
    selectedProjectsGraph: await selectProjectsToVerify(commandOpts),
  }

  const { upToDate, issue, workspaceState } = await checkDepsStatus(opts)
  if (await installNotRequired(opts, upToDate, workspaceState)) return

  const command = ['install', ...createInstallArgs(workspaceState?.settings), ...createFilterArgs(opts)]
  const install = lockedInstall.bind(null, opts, command)

  switch (opts.verifyDepsBeforeRun) {
    case 'install':
      await install()
      break
    case 'prompt':
      if (await confirmInstall(opts, issue, command)) {
        await install()
      }
      break
    case 'error':
      throw new PnpmError('VERIFY_DEPS_BEFORE_RUN', issue ?? 'Your node_modules are out of sync with your lockfile', {
        hint: 'Run "pnpm install"',
      })
    case 'warn':
      globalWarn(`Your node_modules are out of sync with your lockfile. ${issue}`)
      break
    case false:
    case undefined:
      break
  }
}

async function confirmInstall (opts: RunDepsStatusCheckOptions, issue: string | undefined, command: string[]): Promise<boolean> {
  // In non-TTY environments (like CI), we can't prompt the user
  // Exit with error to alert users that node_modules are out of sync
  if (!process.stdin.isTTY) {
    refuseInstallDroppingIgnoredSettings(opts)
    throw new PnpmError('VERIFY_DEPS_BEFORE_RUN', issue ?? 'Your node_modules are out of sync with your lockfile', {
      hint: 'Run "pnpm install" before running scripts. The "verifyDepsBeforeRun: prompt" setting cannot prompt for confirmation in non-interactive environments.',
    })
  }
  try {
    return await confirm({
      message: `Your "node_modules" directory is out of sync with the "pnpm-lock.yaml" file. This can lead to issues during scripts execution.

Would you like to run "pnpm ${command.join(' ')}" to update your "node_modules"?`,
      default: true,
    })
  } catch (err: unknown) {
    if (isError(err) && err.name === 'ExitPromptError') {
      // eslint-disable-next-line n/no-process-exit -- the user cancelled the prompt, so the command ends without an error report
      process.exit(1)
    }
    throw err
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

async function installNotRequired (
  opts: RunDepsStatusCheckOptions,
  upToDate: boolean | undefined,
  workspaceState: CheckDepsStatusResult['workspaceState']
): Promise<boolean> {
  if (!needsInstall(upToDate, opts)) return true
  // Nothing was installed here yet. Spawning an install for projects that
  // give it nothing to do would only leave a lockfile and node_modules behind.
  if (workspaceState != null) return false
  return projectsHaveNothingToInstall(opts)
}

/**
 * Whether an install of a never-installed tree would have nothing to do.
 * `true` only when every project the install would cover declares no
 * dependency, no peer that `autoInstallPeers` fetches, and no install script
 * that runs. A loaded pnpmfile's `readPackage` hook can add dependencies, and
 * a workspace that cannot be read may hide some, so both count as install work.
 * So does a `lockfileDir` pinned outside the project, whose importers this
 * check does not resolve.
 */
async function projectsHaveNothingToInstall (opts: RunDepsStatusCheckOptions): Promise<boolean> {
  if (opts.pnpmfile.length > 0 || lockfileDirIsPinned(opts)) return false
  try {
    return await coveredProjectsHaveNothingToInstall(opts)
  } catch {
    return false
  }
}

/**
 * Whether `lockfileDir` differs from where the install would keep the
 * lockfile: the workspace root under a shared lockfile, otherwise the
 * project the command runs in.
 */
function lockfileDirIsPinned (opts: RunDepsStatusCheckOptions): boolean {
  if (opts.lockfileDir == null) return false
  const defaultLockfileDir = opts.sharedWorkspaceLockfile === false ? opts.dir : (opts.workspaceDir ?? opts.dir)
  return path.resolve(opts.lockfileDir) !== path.resolve(defaultLockfileDir)
}

/**
 * With separate lockfiles, a non-recursive install covers only the project
 * the command runs in. Otherwise it covers the whole workspace, which a
 * non-recursive command does not get as `allProjects`.
 */
async function coveredProjectsHaveNothingToInstall (opts: RunDepsStatusCheckOptions): Promise<boolean> {
  if (opts.sharedWorkspaceLockfile === false && opts.workspaceDir != null && opts.allProjects == null) {
    const manifest = await safeReadProjectManifestOnly(opts.dir)
    return manifest == null || !(
      projectHasInstallWork(opts.dir, manifest, opts) ||
      (path.resolve(opts.dir) === path.resolve(opts.workspaceDir) && runsScript(manifest, 'pnpm:devPreinstall', opts))
    )
  }
  const root = opts.rootProjectManifest
  if (
    root != null &&
    opts.rootProjectManifestDir != null &&
    (projectHasInstallWork(opts.rootProjectManifestDir, root, opts) || runsScript(root, 'pnpm:devPreinstall', opts))
  ) return false
  const projects = opts.allProjects ?? (
    opts.workspaceDir == null
      ? []
      : await findWorkspaceProjectsNoCheck(opts.workspaceDir, {
        patterns: opts.workspacePackagePatterns ?? ['.'],
        modulesDir: opts.modulesDir,
        modulesDirsByProjectName: opts.modulesDirsByProjectName,
      })
  )
  return projects.every(({ rootDir, manifest }) => !projectHasInstallWork(rootDir, manifest, opts))
}

function projectHasInstallWork (rootDir: string, manifest: ProjectManifest, opts: RunDepsStatusCheckOptions): boolean {
  return [manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies]
    .some(group => group != null && Object.keys(group).length > 0) ||
    (opts.autoInstallPeers === true && hasRequiredPeers(manifest)) ||
    PROJECT_LIFECYCLE_STAGES.some(stage => runsScript(manifest, stage, opts)) ||
    (!opts.ignoreScripts && hasImplicitGypBuild(rootDir, manifest))
}

/**
 * Whether the manifest declares at least one peer that is not marked
 * optional. The caller decides whether `autoInstallPeers` fetches them.
 */
function hasRequiredPeers (manifest: ProjectManifest): boolean {
  return Object.keys(manifest.peerDependencies ?? {})
    .some(name => manifest.peerDependenciesMeta?.[name]?.optional !== true)
}

function runsScript (manifest: ProjectManifest, stage: string, opts: RunDepsStatusCheckOptions): boolean {
  return !opts.ignoreScripts && manifest.scripts?.[stage] != null
}

/**
 * A binding.gyp gets an implicit `node-gyp rebuild` install script.
 */
function hasImplicitGypBuild (rootDir: string, manifest: ProjectManifest): boolean {
  return manifest.gypfile !== false && existsSync(path.join(rootDir, 'binding.gyp'))
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
    runInstall(opts, command)
  } finally {
    await lock?.release()
  }
}

/**
 * An install that exits with an error only warns, so a sandbox with a
 * read-only store or no network can still run its scripts. The install has
 * already reported why it failed. An install killed by a signal or by Ctrl-C
 * still aborts the command.
 */
function runInstall (opts: RunDepsStatusCheckOptions, command: string[]): void {
  const loglevel = opts.loglevel === 'silent' || opts.loglevel === 'error' || opts.loglevel === 'warn' ? opts.loglevel : undefined
  try {
    runPnpmCli([...command, ...createCliConfigArgs(opts)], { cwd: opts.dir, loglevel, reporter: opts.reporter })
  } catch (err: unknown) {
    if (!isFailedInstall(err)) throw err
    globalWarn('The install that runs before scripts failed, so your node_modules may be out of sync with your lockfile. Set "verifyDepsBeforeRun: false" to skip this install.')
  }
}

// The exit code Windows reports for a process ended by Ctrl+C.
const STATUS_CONTROL_C_EXIT = 0xC000_013A

function isFailedInstall (err: unknown): boolean {
  return typeof err === 'object' && err != null && 'exitCode' in err &&
    typeof err.exitCode === 'number' && err.exitCode !== STATUS_CONTROL_C_EXIT
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
 * install every project of the workspace.
 */
export function createFilterArgs (opts: Pick<RunDepsStatusCheckOptions, 'filter' | 'filterProd'>): string[] {
  return [
    ...(opts.filter ?? []).map((selector) => `--filter=${withDependencies(selector)}`),
    ...(opts.filterProd ?? []).map((selector) => `--filter-prod=${withDependencies(selector)}`),
  ]
}

/**
 * The command line's `--config.*` options, which the install inherits. They
 * stay out of the prompt, which could otherwise print a credential such as
 * `--config.//registry.example/:_authToken=...`. `dir` is left out: the install
 * starts in the resolved directory, where a relative value would resolve again.
 */
export function createCliConfigArgs (opts: Pick<RunDepsStatusCheckOptions, 'rawCliConfig'>): string[] {
  if (!opts.rawCliConfig) return []
  return Object.entries(opts.rawCliConfig).flatMap(([key, value]) => {
    if (key === 'dir' || value === undefined || value === null) return []
    const values = Array.isArray(value) ? value : [value]
    return values.map((item) => `--config.${key}=${item}`)
  })
}
