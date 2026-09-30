import { packageManager } from '@pnpm/cli.meta'
import { type Config, type ConfigContext, getPackageManagerBootstrapConfig, shouldPersistLockfile } from '@pnpm/config.reader'
import { maturePnpmVersionForRange } from '@pnpm/engine.pm.commands'
import { isPackageManagerResolved, resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { globalWarn } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store.connection-manager'
import semver from 'semver'

import { describeFailure } from './describeFailure.js'

/**
 * Records the currently running pnpm version (see {@link pnpmVersionToRecord})
 * in the env lockfile's `packageManagerDependencies` entry when the project
 * opts in to lockfile-pinned versioning (via `devEngines.packageManager`, or a
 * v12+ `packageManager` pin) and the lockfile doesn't already record a version
 * that satisfies the wanted range.
 *
 * The currently running pnpm version has already been verified by
 * checkPackageManager to satisfy the wanted range.
 *
 * No-op when the project does not pin a pnpm version, when lockfile writing
 * is turned off, or when the recorded entry both satisfies the wanted range
 * and pins every package that version is installed from.
 */
export async function syncEnvLockfile (config: Config, context: ConfigContext): Promise<void> {
  const pm = context.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return
  if (!shouldPersistLockfile(pm)) return
  // The entry lives in pnpm-lock.yaml, which `lockfile: false` opts the
  // project out of (pnpm/pnpm#14728).
  if (config.useLockfile === false) return
  // The currently running pnpm must satisfy the wanted range. Otherwise,
  // recording it in the lockfile would cement an incompatible resolution —
  // checkPackageManager has already surfaced the mismatch to the user.
  if (!semver.satisfies(packageManager.version, pm.version, { includePrerelease: true })) return

  const envLockfile = await readEnvLockfile(context.rootProjectManifestDir) ?? undefined
  const lockedVersion = envLockfile?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (
    lockedVersion != null &&
    semver.satisfies(lockedVersion, pm.version, { includePrerelease: true }) &&
    isPackageManagerResolved(envLockfile, lockedVersion)
  ) return

  const version = await pnpmVersionToRecord(config, pm.version)
  if (version == null) return
  const packageManagerConfig = getPackageManagerBootstrapConfig(config)
  const store = await createStoreController({ ...config, ...context, ...packageManagerConfig, skipBypassedHomeStoreWarning: true })
  try {
    await resolvePackageManagerIntegrities(version, {
      envLockfile,
      registriesByScope: packageManagerConfig.registriesByScope,
      rootDir: context.rootProjectManifestDir,
      storeController: store.ctrl,
      storeDir: store.dir,
      save: true,
      frozenLockfile: config.frozenLockfile,
    })
  } finally {
    await store.ctrl.close()
  }
}

/**
 * The version to record for a pin the running pnpm satisfies. A range pin
 * records the running pnpm only when it meets the project's
 * `minimumReleaseAge`. A frozen lockfile records nothing new, so the lookup
 * is skipped there. `undefined` when the lookup fails: nothing is recorded.
 */
export async function pnpmVersionToRecord (config: Config, wantedVersion: string): Promise<string | undefined> {
  if (config.frozenLockfile || semver.valid(wantedVersion) != null) return packageManager.version
  try {
    return await maturePnpmVersionForRange(config, wantedVersion)
  } catch (err: unknown) {
    // Recording the running pnpm unchecked could pin a release every other
    // contributor's switch refuses. The next command retries the lookup.
    globalWarn(`Skipped recording pnpm v${packageManager.version} in pnpm-lock.yaml because it could not be checked against minimumReleaseAge: ${describeFailure(err)}`)
    return undefined
  }
}
