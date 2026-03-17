import path from 'node:path'

import { packageManager } from '@pnpm/cli-meta'
import type { Config } from '@pnpm/config'
import { isPackageManagerResolved, resolvePackageManagerIntegrities } from '@pnpm/config.deps-installer'
import { prependDirsToPath } from '@pnpm/env.path'
import { PnpmError } from '@pnpm/error'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { globalWarn } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store-connection-manager'
import { installPnpmToStore } from '@pnpm/tools.plugin-commands-self-updater'
import spawn from 'cross-spawn'
import semver from 'semver'

export async function switchCliVersion (config: Config): Promise<void> {
  const pm = config.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return

  let envLockfile = await readEnvLockfile(config.rootProjectManifestDir) ?? undefined
  let storeToUse: Awaited<ReturnType<typeof createStoreController>> | undefined

  // Check if the env lockfile already has a resolved version that satisfies the wanted version/range.
  let pmVersion = envLockfile?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  if (!pmVersion || !semver.satisfies(pmVersion, pm.version, { includePrerelease: true })) {
    // Resolve to an exact version from the registry.
    storeToUse = await createStoreController(config)
    envLockfile = await resolvePackageManagerIntegrities(pm.version, {
      envLockfile,
      registries: config.registries,
      rootDir: config.rootProjectManifestDir,
      storeController: storeToUse.ctrl,
      storeDir: storeToUse.dir,
    })
    pmVersion = envLockfile.importers['.'].packageManagerDependencies?.['pnpm']?.version
    if (!pmVersion) {
      globalWarn(`Cannot resolve pnpm version for "${pm.version}"`)
      await storeToUse?.ctrl.close()
      return
    }
  } else if (!isPackageManagerResolved(envLockfile, pmVersion)) {
    storeToUse = await createStoreController(config)
    envLockfile = await resolvePackageManagerIntegrities(pmVersion, {
      envLockfile,
      registries: config.registries,
      rootDir: config.rootProjectManifestDir,
      storeController: storeToUse.ctrl,
      storeDir: storeToUse.dir,
    })
  }

  // If the wanted version matches the current version, no switch needed
  if (pmVersion === packageManager.version) {
    await storeToUse?.ctrl.close()
    return
  }

  // We need a store controller to install pnpm. If it wasn't created during
  // integrity resolution (because integrities were already cached), create it now.
  if (!storeToUse) {
    storeToUse = await createStoreController(config)
  }

  if (!envLockfile) {
    throw new PnpmError('NO_PKG_MANAGER_INTEGRITY', `The packageManager dependency ${pmVersion} was not found in pnpm-lock.yaml`)
  }

  const { binDir: wantedPnpmBinDir } = await installPnpmToStore(pmVersion, {
    envLockfile,
    storeController: storeToUse.ctrl,
    storeDir: storeToUse.dir,
    registries: config.registries,
    virtualStoreDirMaxLength: config.virtualStoreDirMaxLength,
    packageManager: { name: packageManager.name, version: packageManager.version },
  })

  await storeToUse.ctrl.close()

  const pnpmEnv = prependDirsToPath([wantedPnpmBinDir])
  if (!pnpmEnv.updated) {
    // We throw this error to prevent an infinite recursive call of the same pnpm version.
    throw new VersionSwitchFail(pmVersion, wantedPnpmBinDir)
  }

  // Specify the exact pnpm file path that's expected to execute to spawn.sync()
  //
  // It's not safe spawn 'pnpm' (without specifying an absolute path) and expect
  // it to resolve to the same file path computed above due to the $PATH
  // environment variable. While that does happen in most cases, there's a
  // scenario where the wanted pnpm bin dir exists, but no pnpm binary is
  // present within that directory. If that's the case, a different pnpm bin can
  // get executed, causing infinite spawn and fork bombing the user. See details
  // at https://github.com/pnpm/pnpm/pull/8679.
  const pnpmBinPath = path.join(wantedPnpmBinDir, 'pnpm')

  const { status, signal, error } = spawn.sync(pnpmBinPath, process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
      npm_config_manage_package_manager_versions: 'false',
    },
  })

  if (error) {
    throw new VersionSwitchFail(pmVersion, wantedPnpmBinDir, error)
  }

  if (signal) {
    process.kill(process.pid, signal)
    return
  }

  process.exit(status ?? 0)
}

class VersionSwitchFail extends PnpmError {
  constructor (version: string, wantedPnpmBinDir: string, cause?: unknown) {
    super(
      'VERSION_SWITCH_FAIL',
      `Failed to switch pnpm to v${version}. Looks like pnpm CLI is missing at "${wantedPnpmBinDir}" or is incorrect`,
      { hint: cause instanceof Error ? cause?.message : undefined })

    if (cause != null) {
      this.cause = cause
    }
  }
}
