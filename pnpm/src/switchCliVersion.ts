import { packageManager } from '@pnpm/cli.meta'
import { type Config, type ConfigContext, shouldPersistLockfile } from '@pnpm/config.reader'
import { installPnpmToStore } from '@pnpm/engine.pm.commands'
import { PnpmError } from '@pnpm/error'
import { isPackageManagerResolved, resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { globalWarn } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store.connection-manager'
import semver from 'semver'

import { assertPackageManagerLockfileUsesRegistryResolutions } from './packageManagerLockfile.js'
import { getPackageManagerBootstrapConfig } from './packageManagerRegistries.js'
import { reExecPnpm } from './reExecPnpm.js'

export async function switchCliVersion (config: Config, context: ConfigContext): Promise<void> {
  const pm = context.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return

  const persistLockfile = shouldPersistLockfile(pm)

  // In non-persist mode the env lockfile is intentionally not read, so there
  // is no cached resolution to compare against. Since the legacy
  // `packageManager` field always carries an exact version, we can skip both
  // resolution and store access when the running CLI already matches.
  if (!persistLockfile && pm.version === packageManager.version) return

  let envLockfile = persistLockfile
    ? (await readEnvLockfile(context.rootProjectManifestDir) ?? undefined)
    : undefined
  let storeToUse: Awaited<ReturnType<typeof createStoreController>> | undefined
  const packageManagerConfig = getPackageManagerBootstrapConfig(config)

  // Check if the env lockfile already has a resolved version that satisfies the wanted version/range.
  let pmVersion = envLockfile?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (!pmVersion || !semver.satisfies(pmVersion, pm.version, { includePrerelease: true })) {
    // Resolve to an exact version from the registry.
    storeToUse = await createStoreController({ ...config, ...context, ...packageManagerConfig })
    envLockfile = await resolvePackageManagerIntegrities(pm.version, {
      envLockfile,
      registries: packageManagerConfig.registries,
      rootDir: context.rootProjectManifestDir,
      storeController: storeToUse.ctrl,
      storeDir: storeToUse.dir,
      save: persistLockfile,
    })
    pmVersion = envLockfile.importers['.'].packageManagerDependencies?.['pnpm']?.version
    if (!pmVersion) {
      globalWarn(`Cannot resolve pnpm version for "${pm.version}"`)
      await storeToUse.ctrl.close()
      return
    }
  } else if (!isPackageManagerResolved(envLockfile, pmVersion)) {
    storeToUse = await createStoreController({ ...config, ...context, ...packageManagerConfig })
    envLockfile = await resolvePackageManagerIntegrities(pmVersion, {
      envLockfile,
      registries: packageManagerConfig.registries,
      rootDir: context.rootProjectManifestDir,
      storeController: storeToUse.ctrl,
      storeDir: storeToUse.dir,
      save: persistLockfile,
    })
  }

  // If the wanted version matches the current version, no switch needed.
  // Skip install-to-store entirely — we're already running this version.
  if (pmVersion === packageManager.version) {
    await storeToUse?.ctrl.close()
    return
  }

  if (!envLockfile) {
    await storeToUse?.ctrl.close()
    throw new PnpmError('NO_PKG_MANAGER_INTEGRITY', `The packageManager dependency ${pmVersion} was not found in pnpm-lock.yaml`)
  }

  try {
    assertPackageManagerLockfileUsesRegistryResolutions(envLockfile)
  } catch (err: unknown) {
    await storeToUse?.ctrl.close()
    throw err
  }

  // We need a store controller to install pnpm. If it wasn't created during
  // integrity resolution (because integrities were already cached), create it now.
  if (!storeToUse) {
    storeToUse = await createStoreController({ ...config, ...context, ...packageManagerConfig })
  }

  let wantedPnpmBinDir: string
  try {
    ;({ binDir: wantedPnpmBinDir } = await installPnpmToStore(pmVersion, {
      envLockfile,
      storeController: storeToUse.ctrl,
      storeDir: storeToUse.dir,
      registries: packageManagerConfig.registries,
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
    }))
  } finally {
    await storeToUse.ctrl.close()
  }

  await reExecPnpm(wantedPnpmBinDir, {
    target: `v${pmVersion}`,
    extraEnv: { npm_config_manage_package_manager_versions: 'false' },
  })
}
