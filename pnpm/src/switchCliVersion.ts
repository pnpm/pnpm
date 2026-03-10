import path from 'path'
import { packageManager } from '@pnpm/cli-meta'
import type { Config } from '@pnpm/config'
import { resolvePackageManagerIntegrities } from '@pnpm/config.deps-installer'
import { PnpmError } from '@pnpm/error'
import { prependDirsToPath } from '@pnpm/env.path'
import { globalWarn } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store-connection-manager'
import { installPnpmToStore } from '@pnpm/tools.plugin-commands-self-updater'
import spawn from 'cross-spawn'
import semver from 'semver'

export async function switchCliVersion (config: Config): Promise<void> {
  const pm = config.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return
  const pmVersion = semver.valid(pm.version)
  if (!pmVersion) {
    globalWarn(`Cannot switch to pnpm@${pm.version}: "${pm.version}" is not a valid version`)
    return
  }
  if (pmVersion !== pm.version.trim()) {
    globalWarn(`Cannot switch to pnpm@${pm.version}: you need to specify the version as "${pmVersion}"`)
    return
  }

  const store = await createStoreController(config)

  // Always resolve integrities to populate packageManagerDependencies in the env lockfile
  const envLockfile = await resolvePackageManagerIntegrities(pmVersion, {
    registries: config.registries,
    rootDir: config.rootProjectManifestDir,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  // If the wanted version matches the current version, no switch needed
  if (pmVersion === packageManager.version) return

  const { binDir: wantedPnpmBinDir } = await installPnpmToStore(pmVersion, {
    envLockfile,
    storeController: store.ctrl,
    storeDir: store.dir,
    registries: config.registries,
    virtualStoreDirMaxLength: config.virtualStoreDirMaxLength,
    packageManager: { name: packageManager.name, version: packageManager.version },
  })
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
