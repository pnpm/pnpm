import path from 'node:path'

import { packageManager } from '@pnpm/cli.meta'
import { type Config, type ConfigContext, getPackageManagerBootstrapConfig, shouldPersistLockfile } from '@pnpm/config.reader'
import { assertReleaseIsInstallable, installPnpmToStore, spawnPnpm } from '@pnpm/engine.pm.commands'
import { isError, PnpmError } from '@pnpm/error'
import { isPackageManagerResolved, resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store.connection-manager'
import semver from 'semver'

import { exit } from './exit.js'
import { assertPackageManagerLockfileUsesRegistryResolutions } from './packageManagerLockfile.js'
import { pnpmVersionToRecord } from './syncEnvLockfile.js'

export async function switchCliVersion (config: Config, context: ConfigContext): Promise<void> {
  const pm = context.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return

  const wantedVersion = pm.version

  // `lockfile: false` opts the project out of pnpm-lock.yaml, so the version
  // this switch resolves has nowhere in the project to persist to. Switching
  // itself still happens (pnpm/pnpm#14728).
  const persistLockfile = shouldPersistLockfile(pm) && config.useLockfile !== false

  // In non-persist mode the env lockfile is intentionally not read, so there
  // is no recorded resolution to prefer over the running CLI. Whenever the
  // running CLI satisfies the pin it is the one the project uses, so both
  // resolution and store access can be skipped.
  if (!persistLockfile && satisfiesPin(packageManager.version, wantedVersion)) return

  const envLockfile = persistLockfile
    ? (await readEnvLockfile(context.rootProjectManifestDir) ?? undefined)
    : undefined
  const versionSwitch: VersionSwitch = {
    config,
    context,
    wantedVersion,
    persistLockfile,
    packageManagerConfig: getPackageManagerBootstrapConfig(config),
    envLockfile,
    freshlyResolved: false,
  }

  const pmVersion = await resolveSwitchTargetVersion(versionSwitch)
  if (pmVersion == null) return

  // If the wanted version matches the current version, no switch needed.
  // Skip install-to-store entirely — we're already running this version.
  if (pmVersion === packageManager.version) {
    await versionSwitch.store?.ctrl.close()
    return
  }

  const target = await verifySwitchTarget(versionSwitch, pmVersion)
  if (target == null) return
  await installAndSpawnPnpm(versionSwitch, target)
}

type StoreController = Awaited<ReturnType<typeof createStoreController>>

interface VersionSwitch {
  config: Config
  context: ConfigContext
  wantedVersion: string
  persistLockfile: boolean
  packageManagerConfig: ReturnType<typeof getPackageManagerBootstrapConfig>
  envLockfile: EnvLockfile | undefined
  store?: StoreController
  freshlyResolved: boolean
}

interface SwitchTarget {
  version: string
  envLockfile: EnvLockfile
}

function satisfiesPin (version: string, wantedVersion: string): boolean {
  return semver.satisfies(version, wantedVersion, { includePrerelease: true })
}

/**
 * The exact pnpm version the pin resolves to, with its integrities recorded in
 * `versionSwitch.envLockfile`. Returns `undefined`, after warning and closing
 * the store, when the registry has no version matching the pin.
 */
async function resolveSwitchTargetVersion (versionSwitch: VersionSwitch): Promise<string | undefined> {
  const { config, envLockfile, wantedVersion } = versionSwitch
  let pmVersion = envLockfile?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (pmVersion != null && !satisfiesPin(pmVersion, wantedVersion)) {
    pmVersion = undefined
  }
  // A range pin names no exact version, so the running pnpm's version is the
  // one the project actually uses. Asking the registry instead would pin a
  // version nobody is running and switch away from a satisfying one.
  if (pmVersion == null && satisfiesPin(packageManager.version, wantedVersion)) {
    const version = await pnpmVersionToRecord(config, wantedVersion)
    if (version != null) await recordPin(versionSwitch, version)
    return packageManager.version
  }
  if (pmVersion == null) {
    return resolvePinFromRegistry(versionSwitch)
  }
  await recordPin(versionSwitch, pmVersion)
  return pmVersion
}

async function recordPin (versionSwitch: VersionSwitch, pmVersion: string): Promise<void> {
  const { config, envLockfile, wantedVersion } = versionSwitch
  if (isPackageManagerResolved(envLockfile, pmVersion, config.frozenLockfile ? undefined : wantedVersion)) return
  await resolveIntegrities(versionSwitch, pmVersion, {
    save: versionSwitch.persistLockfile,
    frozenLockfile: config.frozenLockfile,
    specifier: wantedVersion,
  })
  versionSwitch.freshlyResolved = true
}

async function resolvePinFromRegistry (versionSwitch: VersionSwitch): Promise<string | undefined> {
  const resolved = await resolveIntegrities(versionSwitch, versionSwitch.wantedVersion, {
    save: versionSwitch.persistLockfile,
    frozenLockfile: versionSwitch.config.frozenLockfile,
  })
  versionSwitch.freshlyResolved = true
  const pmVersion = resolved.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (!pmVersion) {
    globalWarn(`Cannot resolve pnpm version for "${versionSwitch.wantedVersion}"`)
    await versionSwitch.store?.ctrl.close()
    return undefined
  }
  return pmVersion
}

type ResolveIntegritiesOptions = Parameters<typeof resolvePackageManagerIntegrities>[1]

async function resolveIntegrities (
  versionSwitch: VersionSwitch,
  version: string,
  extraOptions: Pick<ResolveIntegritiesOptions, 'save' | 'frozenLockfile' | 'specifier'>
): Promise<EnvLockfile> {
  versionSwitch.store ??= await createPackageManagerStore(versionSwitch)
  versionSwitch.envLockfile = await resolvePackageManagerIntegrities(version, {
    envLockfile: versionSwitch.envLockfile,
    registriesByScope: versionSwitch.packageManagerConfig.registriesByScope,
    rootDir: versionSwitch.context.rootProjectManifestDir,
    storeController: versionSwitch.store.ctrl,
    storeDir: versionSwitch.store.dir,
    ...extraOptions,
  })
  return versionSwitch.envLockfile
}

async function createPackageManagerStore ({ config, context, packageManagerConfig }: VersionSwitch): Promise<StoreController> {
  return createStoreController({ ...config, ...context, ...packageManagerConfig, skipBypassedHomeStoreWarning: true })
}

/**
 * Checks that the version to switch to may be installed from the env
 * lockfile, repairing lockfile entries that do not satisfy the bootstrap
 * rules. Returns `undefined` when no switch should happen. The store is closed
 * on every path that does not lead to a switch.
 */
async function verifySwitchTarget (versionSwitch: VersionSwitch, pmVersion: string): Promise<SwitchTarget | undefined> {
  // Deliberately after the version-match check: switching to a broken release is
  // refused, but running one already installed is not. Someone whose pnpm is a
  // broken release still needs it to work well enough to move off it.
  try {
    assertReleaseIsInstallable(pmVersion)
  } catch (err: unknown) {
    await versionSwitch.store?.ctrl.close()
    throw err
  }

  const { envLockfile } = versionSwitch
  if (!envLockfile) {
    await versionSwitch.store?.ctrl.close()
    throw new PnpmError('NO_PKG_MANAGER_INTEGRITY', `The packageManager dependency ${pmVersion} was not found in pnpm-lock.yaml`)
  }

  try {
    return await ensureRegistryResolutions(versionSwitch, { version: pmVersion, envLockfile })
  } catch (err: unknown) {
    await versionSwitch.store?.ctrl.close()
    throw err
  }
}

async function ensureRegistryResolutions (versionSwitch: VersionSwitch, target: SwitchTarget): Promise<SwitchTarget | undefined> {
  try {
    assertPackageManagerLockfileUsesRegistryResolutions(target.envLockfile)
    return target
  } catch (err: unknown) {
    if (versionSwitch.freshlyResolved || !isInvalidPackageManagerLockfileError(err)) {
      throw err
    }
    return repairPackageManagerLockfile(versionSwitch, target)
  }
}

function isInvalidPackageManagerLockfileError (err: unknown): boolean {
  return isError(err) && 'code' in err && err.code === 'ERR_PNPM_INVALID_PACKAGE_MANAGER_LOCKFILE'
}

/**
 * The persisted entries do not satisfy the bootstrap rules — a
 * resolution carrying a tarball URL, say. Rather than refusing to run,
 * discard them and resolve afresh through the trusted bootstrap
 * registries, which yields entries in the accepted shape.
 *
 * They already record a version that satisfies the pin, so a frozen
 * lockfile has nothing to reject: the repair resolves that version
 * rather than the range around it, keeps the result in memory, and
 * leaves the lockfile as it is.
 */
async function repairPackageManagerLockfile (versionSwitch: VersionSwitch, target: SwitchTarget): Promise<SwitchTarget | undefined> {
  const { config, wantedVersion } = versionSwitch
  delete target.envLockfile.importers['.'].packageManagerDependencies
  const envLockfile = await resolveIntegrities(versionSwitch, config.frozenLockfile ? target.version : wantedVersion, {
    save: versionSwitch.persistLockfile && !config.frozenLockfile,
  })
  const pmVersion = envLockfile.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (!pmVersion) {
    globalWarn(`Cannot resolve pnpm version for "${wantedVersion}"`)
    await versionSwitch.store?.ctrl.close()
    return undefined
  }
  if (pmVersion === packageManager.version) {
    await versionSwitch.store?.ctrl.close()
    return undefined
  }
  assertReleaseIsInstallable(pmVersion)
  assertPackageManagerLockfileUsesRegistryResolutions(envLockfile)
  return { version: pmVersion, envLockfile }
}

async function installAndSpawnPnpm (versionSwitch: VersionSwitch, target: SwitchTarget): Promise<void> {
  // We need a store controller to install pnpm. If it wasn't created during
  // integrity resolution (because integrities were already cached), create it now.
  const store = versionSwitch.store ?? await createPackageManagerStore(versionSwitch)

  let wantedPnpmBinDir: string
  try {
    ;({ binDir: wantedPnpmBinDir } = await installPnpmToStore(target.version, installPnpmToStoreOptions(versionSwitch.config, target.envLockfile, store)))
  } finally {
    await store.ctrl.close()
  }

  // Specify the exact pnpm file path that's expected to execute to spawn()
  //
  // It's not safe spawn 'pnpm' (without specifying an absolute path) and expect
  // it to resolve to the same file path computed above due to the $PATH
  // environment variable. While that does happen in most cases, there's a
  // scenario where the wanted pnpm bin dir exists, but no pnpm binary is
  // present within that directory. If that's the case, a different pnpm bin can
  // get executed, causing infinite spawn and fork bombing the user. See details
  // at https://github.com/pnpm/pnpm/pull/8679.
  const pnpmBinPath = path.join(wantedPnpmBinDir, 'pnpm')

  let status: number | null
  let signal: NodeJS.Signals | null
  try {
    ;({ status, signal } = await spawnPnpm(pnpmBinPath, process.argv.slice(2)))
  } catch (err: unknown) {
    throw new VersionSwitchFail(target.version, wantedPnpmBinDir, err)
  }

  if (signal) {
    process.kill(process.pid, signal)
    return
  }

  await exit(status ?? 0)
}

/**
 * Install the pnpm that the env lockfile pins into the store, when a command
 * in the project would switch to it. `pnpm fetch` reads only the lockfile, and
 * a later `pnpm install --offline` that switches to the pinned pnpm finds it
 * there instead of in the registry (pnpm/pnpm#11808).
 */
export async function fetchLockedPackageManager (config: Config, context: ConfigContext): Promise<void> {
  if (config.pmOnFail != null && config.pmOnFail !== 'download') return
  const envLockfile = await readEnvLockfile(context.rootProjectManifestDir)
  const pmVersion = envLockfile?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (
    envLockfile == null ||
    pmVersion == null ||
    pmVersion === packageManager.version ||
    !isPackageManagerResolved(envLockfile, pmVersion)
  ) return
  try {
    assertPackageManagerLockfileUsesRegistryResolutions(envLockfile)
  } catch (err: unknown) {
    // Entries in another shape are re-resolved from the registry by the
    // switch itself, so installing them here would not help it offline.
    if (
      !isError(err) ||
      !('code' in err) ||
      err.code !== 'ERR_PNPM_INVALID_PACKAGE_MANAGER_LOCKFILE'
    ) {
      throw err
    }
    return
  }
  assertReleaseIsInstallable(pmVersion)
  const store = await createStoreController({ ...config, ...context, ...getPackageManagerBootstrapConfig(config), skipBypassedHomeStoreWarning: true })
  try {
    await installPnpmToStore(pmVersion, installPnpmToStoreOptions(config, envLockfile, store))
  } finally {
    await store.ctrl.close()
  }
}

function installPnpmToStoreOptions (
  config: Config,
  envLockfile: EnvLockfile,
  store: Awaited<ReturnType<typeof createStoreController>>
): Parameters<typeof installPnpmToStore>[1] {
  return {
    envLockfile,
    storeController: store.ctrl,
    storeDir: store.dir,
    registriesByScope: getPackageManagerBootstrapConfig(config).registriesByScope,
    virtualStoreDirMaxLength: config.virtualStoreDirMaxLength,
    packageManager: { name: packageManager.name, version: packageManager.version },
    // Network settings so the engine identity check can reach the canonical
    // npm registry through the user's proxy / TLS configuration.
    ca: config.ca,
    cert: config.cert,
    key: config.key,
    httpProxy: config.httpProxy,
    httpsProxy: config.httpsProxy,
    noProxy: config.noProxy,
    strictSsl: config.strictSsl,
    localAddress: config.localAddress,
    maxSockets: config.maxSockets,
    configByUri: config.configByUri,
    timeout: config.fetchTimeout,
  }
}

class VersionSwitchFail extends PnpmError {
  constructor (version: string, wantedPnpmBinDir: string, cause?: unknown) {
    super(
      'VERSION_SWITCH_FAIL',
      `Failed to switch pnpm to v${version}. Looks like pnpm CLI is missing at "${wantedPnpmBinDir}" or is incorrect`,
      { hint: isError(cause) ? cause?.message : undefined })

    if (cause != null) {
      this.cause = cause
    }
  }
}
