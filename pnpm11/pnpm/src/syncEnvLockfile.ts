import { packageManager } from '@pnpm/cli.meta'
import { type Config, type ConfigContext, getPackageManagerBootstrapConfig, shouldPersistLockfile } from '@pnpm/config.reader'
import { isPackageManagerResolved, resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { createStoreController } from '@pnpm/store.connection-manager'
import semver from 'semver'

/**
 * Records the currently running pnpm version in the env lockfile's
 * `packageManagerDependencies` entry when the project opts in to
 * lockfile-pinned versioning (via `devEngines.packageManager`, or a v12+
 * `packageManager` pin) and the lockfile doesn't already record a version
 * that satisfies the wanted range.
 *
 * The currently running pnpm version has already been verified by
 * checkPackageManager to satisfy the wanted range, so recording it is safe.
 *
 * No-op when the project does not pin a pnpm version, or when the recorded
 * entry both satisfies the wanted range and pins every package that version
 * is installed from.
 */
export async function syncEnvLockfile (config: Config, context: ConfigContext): Promise<void> {
  const pm = context.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return
  if (!shouldPersistLockfile(pm)) return
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

  const packageManagerConfig = getPackageManagerBootstrapConfig(config)
  const store = await createStoreController({ ...config, ...context, ...packageManagerConfig })
  try {
    await resolvePackageManagerIntegrities(packageManager.version, {
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
