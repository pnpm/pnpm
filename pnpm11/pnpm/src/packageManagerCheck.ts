import { formatWarn } from '@pnpm/cli.default-reporter'
import { isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { getSystemRuntimeVersion } from '@pnpm/engine.runtime.system-version'
import { PnpmError } from '@pnpm/error'
import { type EngineDependency, isRuntimeAlias, type RuntimeName } from '@pnpm/types'
import semver from 'semver'

import { skipPackageManagerCheckForCommand } from './cmd/index.js'
import { describeFailure } from './describeFailure.js'
import { fetchLockedPackageManager, switchCliVersion } from './switchCliVersion.js'
import { syncEnvLockfile } from './syncEnvLockfile.js'

export interface PackageManagerHandlingOptions {
  cmd: string | null
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- option values have command-specific types
  cliOptions: Record<string, any>
  config: Config
  context: ConfigContext
  rejectUnknownOptions: boolean
}

/**
 * Enforces the package manager and runtimes the project pins: switches to the
 * pinned pnpm, or checks the running one against the pin, and checks the
 * system runtimes against the wanted ones.
 */
export async function handlePackageManagerAndRuntimes (opts: PackageManagerHandlingOptions): Promise<void> {
  const { cmd, cliOptions, config, context, rejectUnknownOptions } = opts
  const pm = context.wantedPackageManager
  if (pm != null) {
    if (pm.onFail !== 'ignore') {
      await handleWantedPackageManager(pm, opts)
    }
  } else if (cmd === 'fetch' && !isExecutedByCorepack() && !rejectUnknownOptions) {
    await fetchLockedPackageManager(config, context)
  }
  if (cmd != null && !cliOptions.global && !rejectUnknownOptions) {
    for (const runtime of getWantedRuntimes(context)) {
      checkRuntime(runtime, config)
    }
  }
}

async function handleWantedPackageManager (pm: EngineDependency, opts: PackageManagerHandlingOptions): Promise<void> {
  const { cmd, cliOptions, config, context } = opts
  const printingVersion = cmd == null && cliOptions.version === true
  if (cliOptions.global) {
    // Global state belongs to the pnpm the user invoked, not to the
    // project, so a global command never switches to the pinned pnpm.
    if (!isRunningPnpmPinned(pm)) {
      warnAboutProject(config, 'Using --global skips the package manager check for this project')
    }
    return
  }
  if (pm.name === 'pnpm' && pm.onFail === 'download' && !isExecutedByCorepack()) {
    // Corepack owns version switching; pnpm only switches versions when
    // the user is running pnpm directly.
    await tolerateWhenPrintingVersion(printingVersion, async () => {
      await switchCliVersion(config, context)
    })
    return
  }
  // checkPackageManager and syncEnvLockfile run regardless of how pnpm
  // was invoked. Different developers on the same project may use
  // corepack or invoke pnpm directly, and the lockfile's
  // `packageManagerDependencies` entry must stay consistent across both
  // workflows. syncEnvLockfile self-gates via shouldPersistLockfile so
  // it only writes to the lockfile when the project opted in (via
  // `devEngines.packageManager`, or a v12+ `packageManager` pin).
  checkPackageManager(pm, { config, underCorepack: isExecutedByCorepack() })
  if (!opts.rejectUnknownOptions) {
    await tolerateWhenPrintingVersion(printingVersion, async () => {
      await syncEnvLockfile(config, context)
    })
  }
}

/**
 * Returns whether the command may bypass project package-manager and runtime
 * handling. Config command aliases bypass it unless `location` is exactly
 * `project`; an absent or unrecognized location therefore retains config's
 * global default. Commands marked with `skipPackageManagerCheck`, and help
 * requests targeting those commands, also bypass it. A missing command does
 * not.
 */
export function shouldSkipPmHandling (cmd: string | null, cliParams: string[], location: unknown): boolean {
  if (cmd == null) return false
  if ((cmd === 'config' || cmd === 'c' || cmd === 'get' || cmd === 'set') && location !== 'project') return true
  if (skipPackageManagerCheckForCommand.has(cmd)) return true
  if (cmd === 'help' && cliParams[0] != null && skipPackageManagerCheckForCommand.has(cliParams[0])) return true
  return false
}

/**
 * `pnpm --version` must answer even where the pinned pnpm cannot be installed
 * or recorded: a sandbox with a read-only filesystem leaves pnpm nowhere to
 * write. The failure is reported and the running pnpm's version is printed
 * instead of the pinned one. Checks that reject the project outright, like a
 * pin naming another package manager, still fail the command.
 */
async function tolerateWhenPrintingVersion (printingVersion: boolean, work: () => Promise<void>): Promise<void> {
  try {
    await work()
  } catch (err: unknown) {
    if (!printingVersion) throw err
    // The version prints before the reporter subscribes to the log stream,
    // so this warning goes straight to stderr.
    console.error(formatWarn(`Cannot use the pnpm version this project pins: ${describeFailure(err)}`))
  }
}

function isRunningPnpmPinned (pm: EngineDependency): boolean {
  if (pm.name !== 'pnpm' || packageManager.name !== 'pnpm') return false
  return !pm.version || semver.satisfies(packageManager.version, pm.version, { includePrerelease: true })
}

function checkPackageManager (pm: EngineDependency, opts: { config: Config, underCorepack: boolean }): void {
  if (!pm.name) return
  const shouldError = pm.onFail === 'error' || pm.onFail === 'download'
  if (pm.name !== 'pnpm') {
    const msg = `This project is configured to use ${pm.name}`
    if (shouldError) {
      throw new PnpmError('OTHER_PM_EXPECTED', msg)
    }
    warnAboutProject(opts.config, msg)
    return
  }
  if (!pm.version) return
  const currentPnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.version
    : undefined
  if (!currentPnpmVersion || semver.satisfies(currentPnpmVersion, pm.version, { includePrerelease: true })) return
  reportPnpmVersionMismatch(opts.config, {
    wantedVersion: pm.version,
    currentPnpmVersion,
    shouldError,
    underCorepack: opts.underCorepack,
  })
}

function reportPnpmVersionMismatch (config: Config, mismatch: {
  wantedVersion: string
  currentPnpmVersion: string
  shouldError: boolean
  underCorepack: boolean
}): void {
  let msg = `This project is configured to use ${mismatch.wantedVersion} of pnpm. Your current pnpm is v${mismatch.currentPnpmVersion}`
  // When pnpm runs under corepack, corepack — not pnpm — selects the
  // running version, so users see this mismatch even with onFail='download'
  // (which would normally auto-switch). Spell out that pnpm cannot switch
  // here and point at the two ways out.
  if (mismatch.underCorepack) {
    msg += '\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.'
  }
  if (!mismatch.shouldError) {
    warnAboutProject(config, msg)
    return
  }
  const baseHint = 'If you want to bypass this version check, you can set the "pmOnFail" configuration to "warn" or "ignore" (e.g. via --pm-on-fail=ignore). If using "devEngines.packageManager", you can set its "onFail" to "warn" or "ignore"'
  const hint = mismatch.underCorepack
    ? `Align the "packageManager" field in package.json with "devEngines.packageManager", or invoke pnpm directly (without corepack) so it can switch versions automatically.\n${baseHint}`
    : baseHint
  throw new PnpmError('BAD_PM_VERSION', msg, { hint })
}

const RUNTIME_DISPLAY_NAMES: Record<RuntimeName, string> = {
  node: 'Node.js',
  deno: 'Deno',
  bun: 'Bun',
}

// devEngines.runtime takes precedence over engines.runtime per the iteration
// order below: the first entry seen for a given runtime wins.
function getWantedRuntimes (context: ConfigContext): EngineDependency[] {
  const manifest = context.enginePinManifest
  if (manifest == null) return []
  const result: EngineDependency[] = []
  const seen = new Set<RuntimeName>()
  for (const enginesFieldName of ['devEngines', 'engines'] as const) {
    for (const runtime of listRuntimes(manifest[enginesFieldName]?.runtime)) {
      if (!runtime.name || !isRuntimeAlias(runtime.name) || seen.has(runtime.name)) continue
      seen.add(runtime.name)
      result.push(runtime)
    }
  }
  return result
}

function listRuntimes (enginesRuntime: EngineDependency | EngineDependency[] | undefined): EngineDependency[] {
  if (enginesRuntime == null) return []
  return Array.isArray(enginesRuntime) ? enginesRuntime : [enginesRuntime]
}

function checkRuntime (runtime: EngineDependency, config: Config): void {
  if (runtime.onFail == null || runtime.onFail === 'ignore' || runtime.onFail === 'download') return
  if (!runtime.name || !isRuntimeAlias(runtime.name)) return
  const runtimeName: RuntimeName = runtime.name
  const displayName = RUNTIME_DISPLAY_NAMES[runtimeName]
  const wantedRange = runtime.version
  if (!wantedRange || !semver.validRange(wantedRange)) {
    const msg = wantedRange
      ? `This project requires an invalid ${displayName} version range: ${wantedRange}`
      : `This project requires a ${displayName} runtime but does not specify a version range`
    failRuntimeCheck(config, runtime.onFail, msg)
    return
  }
  const currentVersion = getSystemRuntimeVersion(runtimeName)
  if (currentVersion == null) {
    failRuntimeCheck(
      config,
      runtime.onFail,
      `This project requires ${displayName} ${wantedRange}, but ${displayName} was not found on the system`
    )
    return
  }
  if (semver.satisfies(currentVersion, wantedRange, { includePrerelease: true })) return

  failRuntimeCheck(
    config,
    runtime.onFail,
    `This project requires ${displayName} ${wantedRange}. Your current ${displayName} is ${currentVersion}`
  )
}

function failRuntimeCheck (config: Config, onFail: 'error' | 'warn', message: string): void {
  if (onFail === 'error') {
    throw new PnpmError('BAD_RUNTIME_VERSION', message, { hint: RUNTIME_ON_FAIL_HINT })
  }
  warnAboutProject(config, message)
}

/**
 * These warnings are not the command's output, so they go to stderr: a command
 * such as `pnpm cache path` or `pnpm list --json` prints a value a script reads
 * from stdout. They bypass the reporter, as the ndjson one attaches after these
 * checks and would drop them.
 */
function warnAboutProject (config: Config, message: string): void {
  if (config.loglevel === 'silent' || config.loglevel === 'error' || config.reporter === 'silent') return
  console.warn(formatWarn(message))
}

const RUNTIME_ON_FAIL_HINT = 'If you want to bypass this version check, set "runtimeOnFail" to "warn" or "ignore" (e.g. via --runtime-on-fail=ignore), or set "devEngines.runtime.onFail"/"engines.runtime.onFail" to "warn" or "ignore"'
