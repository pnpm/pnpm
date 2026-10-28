import { isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import { type Config, shouldPersistLockfile } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { isPackageManagerResolved } from '@pnpm/installing.env-installer'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import type { EngineDependency } from '@pnpm/types'
import semver from 'semver'

export type PinEntryPoint = 'command' | 'version' | 'unknown-flag' | 'self-update'

export interface PinInput {
  wanted: EngineDependency | undefined
  lockedEnv?: EnvLockfile
  runningVersion?: string
  runningName?: string
  underCorepack?: boolean
  isResolved?: (envLockfile: EnvLockfile | undefined, pnpmVersion: string, specifier?: string) => boolean
}

export interface PinDecisionSettings {
  pmOnFail?: EngineDependency['onFail']
  useLockfile?: boolean
  frozenLockfile?: boolean
  lockfileDir?: string
  rootProjectManifestDir: string
  loglevel?: Config['loglevel']
  reporter?: Config['reporter']
}

export interface PinInvocation {
  entryPoint: PinEntryPoint
  global?: boolean
  skipPmHandling?: boolean
  recordPinWithoutSwitch: boolean
  warn?: (message: string) => void
}

export type PinDecision =
  | {
    action: 'switch'
    specifier: string
    lockedVersion?: string
    persistLockfile: boolean
    at: string
  }
  | {
    action: 'record'
    at: string
    version: string
    specifier: string
    checkMaturity: boolean
  }
  | {
    action: 'nothing'
  }

interface ResolvedDecisionContext {
  pin: PinInput
  pm: EngineDependency
  settings: PinDecisionSettings
  invocation: PinInvocation
  runningName: string
  runningVersion: string
  underCorepack: boolean
  isResolved: (envLockfile: EnvLockfile | undefined, pnpmVersion: string, specifier?: string) => boolean
}

export function shouldReadPinLockfile (
  pm: EngineDependency | undefined,
  settings: Pick<PinDecisionSettings, 'pmOnFail' | 'useLockfile'>,
  invocation: Partial<Pick<PinInvocation, 'entryPoint' | 'global' | 'recordPinWithoutSwitch'>> = {},
  runningVersion = packageManager.version,
  underCorepack = isExecutedByCorepack()
): boolean {
  if (invocation.global || settings.useLockfile === false || pm == null || pm.name !== 'pnpm' || pm.version == null) {
    return false
  }
  const effectiveOnFail = settings.pmOnFail ?? pm.onFail ?? 'download'
  if (effectiveOnFail === 'ignore' || !shouldPersistLockfile({ ...pm, onFail: effectiveOnFail })) {
    return false
  }
  if (invocation.entryPoint !== 'self-update' && effectiveOnFail === 'download' && !underCorepack) {
    return true
  }
  return invocation.entryPoint !== 'unknown-flag' &&
    invocation.recordPinWithoutSwitch !== false &&
    satisfiesPin(runningVersion, pm.version)
}

/**
 * Pure decision over a pre-read {@link PinInput}, {@link PinDecisionSettings},
 * and {@link PinInvocation}. Performs no filesystem I/O.
 */
export function decide (
  pin: PinInput,
  settings: PinDecisionSettings,
  invocation: PinInvocation
): PinDecision {
  const ctx = resolveDecisionContext(pin, settings, invocation)
  if (ctx == null) return { action: 'nothing' }
  if (invocation.global) return decideGlobal(ctx)
  if (canSwitchPnpm(ctx)) return decideSwitchOrRecord(ctx)
  if (invocation.entryPoint === 'unknown-flag') return { action: 'nothing' }
  checkPackageManager(ctx.pm, ctx)
  return decideRecord(ctx)
}

function resolveDecisionContext (
  pin: PinInput,
  settings: PinDecisionSettings,
  invocation: PinInvocation
): ResolvedDecisionContext | undefined {
  if (invocation.skipPmHandling || !pin.wanted?.name) return undefined
  const onFail = settings.pmOnFail ?? pin.wanted.onFail ?? 'download'
  if (onFail === 'ignore') return undefined
  return {
    pin,
    pm: { ...pin.wanted, onFail },
    settings,
    invocation,
    runningName: pin.runningName ?? packageManager.name,
    runningVersion: pin.runningVersion ?? packageManager.version,
    underCorepack: pin.underCorepack ?? isExecutedByCorepack(),
    isResolved: pin.isResolved ?? isPackageManagerResolved,
  }
}

function decideGlobal (ctx: ResolvedDecisionContext): PinDecision {
  // Global state belongs to the pnpm the user invoked, not to the project,
  // so a global command never switches to the pinned pnpm (pnpm/pnpm#14531).
  if (ctx.invocation.entryPoint !== 'unknown-flag' && !isRunningPnpmPinned(ctx.pm, ctx.runningName, ctx.runningVersion)) {
    ctx.invocation.warn?.('Using --global skips the package manager check for this project')
  }
  return { action: 'nothing' }
}

function canSwitchPnpm (ctx: ResolvedDecisionContext): boolean {
  return ctx.invocation.entryPoint !== 'self-update' &&
    ctx.pm.name === 'pnpm' &&
    ctx.pm.onFail === 'download' &&
    !ctx.underCorepack
}

function decideSwitchOrRecord (ctx: ResolvedDecisionContext): PinDecision {
  const specifier = ctx.pm.version
  if (!specifier) return { action: 'nothing' }
  const at = envLockfileRoot(ctx.settings)
  const persistLockfile = shouldPersistLockfile(ctx.pm) && ctx.settings.useLockfile !== false
  const lockedVersion = persistLockfile ? matchingLockedVersion(ctx.pin.lockedEnv, specifier) : undefined
  if (lockedVersion != null) {
    return decideFromLockedVersion(ctx, { at, lockedVersion, persistLockfile, specifier })
  }
  if (ctx.runningName === 'pnpm' && satisfiesPin(ctx.runningVersion, specifier)) {
    return decideForRunningPnpm(ctx, { at, persistLockfile, specifier })
  }
  return { action: 'switch', specifier, persistLockfile, at }
}

function decideFromLockedVersion (
  ctx: ResolvedDecisionContext,
  { at, lockedVersion, persistLockfile, specifier }: {
    at: string
    lockedVersion: string
    persistLockfile: boolean
    specifier: string
  }
): PinDecision {
  if (lockedVersion !== ctx.runningVersion) {
    return { action: 'switch', specifier, lockedVersion, persistLockfile, at }
  }
  if (!mayRecordWithoutSwitch(ctx, persistLockfile) || isSpecResolved(ctx, lockedVersion, specifier)) {
    return { action: 'nothing' }
  }
  return { action: 'record', at, version: lockedVersion, specifier, checkMaturity: false }
}

function decideForRunningPnpm (
  ctx: ResolvedDecisionContext,
  { at, persistLockfile, specifier }: { at: string, persistLockfile: boolean, specifier: string }
): PinDecision {
  if (!mayRecordWithoutSwitch(ctx, persistLockfile)) return { action: 'nothing' }
  const checkMaturity = !ctx.settings.frozenLockfile && semver.valid(specifier) == null
  if (!checkMaturity && isSpecResolved(ctx, ctx.runningVersion, specifier)) {
    return { action: 'nothing' }
  }
  return { action: 'record', at, version: ctx.runningVersion, specifier, checkMaturity }
}

function mayRecordWithoutSwitch (ctx: ResolvedDecisionContext, persistLockfile: boolean): boolean {
  return ctx.invocation.entryPoint !== 'unknown-flag' &&
    ctx.invocation.recordPinWithoutSwitch &&
    persistLockfile
}

function decideRecord (ctx: ResolvedDecisionContext): PinDecision {
  const specifier = recordableSpecifier(ctx)
  if (!specifier || isAlreadyRecorded(ctx, specifier)) return { action: 'nothing' }
  const checkMaturity = ctx.invocation.entryPoint !== 'self-update' &&
    !ctx.settings.frozenLockfile &&
    semver.valid(specifier) == null
  return {
    action: 'record',
    at: envLockfileRoot(ctx.settings),
    version: ctx.runningVersion,
    specifier,
    checkMaturity,
  }
}

function recordableSpecifier (ctx: ResolvedDecisionContext): string | undefined {
  if (!ctx.invocation.recordPinWithoutSwitch) return undefined
  if (ctx.pm.name !== 'pnpm' || !ctx.pm.version) return undefined
  if (!shouldPersistLockfile(ctx.pm) || ctx.settings.useLockfile === false) return undefined
  if (ctx.runningName !== 'pnpm' || !satisfiesPin(ctx.runningVersion, ctx.pm.version)) return undefined
  return ctx.pm.version
}

function isAlreadyRecorded (ctx: ResolvedDecisionContext, specifier: string): boolean {
  const lockedVersion = matchingLockedVersion(ctx.pin.lockedEnv, specifier)
  if (lockedVersion == null) return false
  if (ctx.invocation.entryPoint !== 'self-update') {
    return ctx.isResolved(ctx.pin.lockedEnv, lockedVersion)
  }
  return lockedVersion === ctx.runningVersion && isSpecResolved(ctx, ctx.runningVersion, specifier)
}

function isSpecResolved (ctx: ResolvedDecisionContext, version: string, specifier: string): boolean {
  return ctx.isResolved(ctx.pin.lockedEnv, version, ctx.settings.frozenLockfile ? undefined : specifier)
}

function envLockfileRoot (settings: PinDecisionSettings): string {
  return settings.lockfileDir ?? settings.rootProjectManifestDir
}

function matchingLockedVersion (lockedEnv: EnvLockfile | undefined, wantedVersion: string): string | undefined {
  const version = lockedEnv?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (version != null && satisfiesPin(version, wantedVersion)) return version
  return undefined
}

function satisfiesPin (version: string, wantedVersion: string): boolean {
  return semver.satisfies(version, wantedVersion, { includePrerelease: true })
}

function isRunningPnpmPinned (pm: EngineDependency, runningName: string, runningVersion: string): boolean {
  if (pm.name !== 'pnpm' || runningName !== 'pnpm') return false
  return !pm.version || satisfiesPin(runningVersion, pm.version)
}

function checkPackageManager (
  pm: EngineDependency,
  opts: {
    invocation: PinInvocation
    runningName: string
    runningVersion: string
    underCorepack: boolean
  }
): void {
  if (!pm.name) return
  const shouldError = pm.onFail === 'error' || pm.onFail === 'download'
  if (pm.name !== 'pnpm') {
    const msg = `This project is configured to use ${pm.name}`
    if (shouldError) {
      throw new PnpmError('OTHER_PM_EXPECTED', msg)
    }
    opts.invocation.warn?.(msg)
    return
  }
  if (!pm.version) return
  const currentPnpmVersion = opts.runningName === 'pnpm'
    ? opts.runningVersion
    : undefined
  if (!currentPnpmVersion || satisfiesPin(currentPnpmVersion, pm.version)) return
  reportPnpmVersionMismatch(opts.invocation, {
    wantedVersion: pm.version,
    currentPnpmVersion,
    shouldError,
    underCorepack: opts.underCorepack,
  })
}

function reportPnpmVersionMismatch (
  invocation: PinInvocation,
  mismatch: {
    wantedVersion: string
    currentPnpmVersion: string
    shouldError: boolean
    underCorepack: boolean
  }
): void {
  let msg = `This project is configured to use ${mismatch.wantedVersion} of pnpm. Your current pnpm is v${mismatch.currentPnpmVersion}`
  // When pnpm runs under corepack, corepack — not pnpm — selects the
  // running version, so users see this mismatch even with onFail='download'
  // (which would normally auto-switch). Spell out that pnpm cannot switch
  // here and point at the two ways out.
  if (mismatch.underCorepack) {
    msg += '\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.'
  }
  if (!mismatch.shouldError) {
    invocation.warn?.(msg)
    return
  }
  const baseHint = 'If you want to bypass this version check, you can set the "pmOnFail" configuration to "warn" or "ignore" (e.g. via --pm-on-fail=ignore). If using "devEngines.packageManager", you can set its "onFail" to "warn" or "ignore"'
  const hint = mismatch.underCorepack
    ? `Align the "packageManager" field in package.json with "devEngines.packageManager", or invoke pnpm directly (without corepack) so it can switch versions automatically.\n${baseHint}`
    : baseHint
  throw new PnpmError('BAD_PM_VERSION', msg, { hint })
}
