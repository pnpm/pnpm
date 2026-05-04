import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { createStoreController } from '@pnpm/store.connection-manager'
import semver from 'semver'

import { shouldPersistLockfile } from './shouldPersistLockfile.js'

/**
 * Refreshes the env lockfile's `packageManagerDependencies` entry when it
 * records a pnpm version that no longer satisfies the wanted
 * `devEngines.packageManager` range. The currently running pnpm version
 * (already verified to satisfy the wanted range by checkPackageManager) is
 * recorded as the new resolution.
 *
 * No-op when the project does not pin a pnpm version, when no env lockfile
 * exists yet, or when the recorded version still satisfies the wanted range.
 */
export async function syncEnvLockfile (config: Config, context: ConfigContext): Promise<void> {
  const pm = context.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return
  if (!shouldPersistLockfile(pm)) return
  // The currently running pnpm must satisfy the wanted range. Otherwise,
  // recording it in the lockfile would cement an incompatible resolution —
  // checkPackageManager has already surfaced the mismatch to the user.
  if (!semver.satisfies(packageManager.version, pm.version, { includePrerelease: true })) return

  const envLockfile = await readEnvLockfile(context.rootProjectManifestDir)
  if (envLockfile == null) return
  const lockedVersion = envLockfile.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (lockedVersion == null) return
  if (semver.satisfies(lockedVersion, pm.version, { includePrerelease: true })) return

  const store = await createStoreController({ ...config, ...context })
  try {
    await resolvePackageManagerIntegrities(packageManager.version, {
      envLockfile,
      registries: config.registries,
      rootDir: context.rootProjectManifestDir,
      storeController: store.ctrl,
      storeDir: store.dir,
      save: true,
    })
  } finally {
    await store.ctrl.close()
  }
}
