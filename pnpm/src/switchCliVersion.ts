import path from 'node:path'

import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { resolveAndInstallPnpmVersion } from '@pnpm/engine.pm.commands'
import { PnpmError } from '@pnpm/error'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { globalWarn } from '@pnpm/logger'
import { prependDirsToPath } from '@pnpm/shell.path'
import { createStoreController } from '@pnpm/store.connection-manager'
import spawn from 'cross-spawn'
import semver from 'semver'

export async function switchCliVersion (config: Config, context: ConfigContext): Promise<void> {
  const pm = context.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null) return

  const existingEnvLockfile = await readEnvLockfile(context.rootProjectManifestDir) ?? undefined
  const cachedVersion = existingEnvLockfile?.importers['.'].packageManagerDependencies?.['pnpm']?.version
  // If a previously resolved version still satisfies the wanted range, reuse
  // it so we don't re-hit the registry for range-based pins. Otherwise, let
  // the resolve step in the helper look the range up again.
  const versionToInstall = (cachedVersion && semver.satisfies(cachedVersion, pm.version, { includePrerelease: true }))
    ? cachedVersion
    : pm.version

  const storeToUse = await createStoreController({ ...config, ...context })
  let result!: Awaited<ReturnType<typeof resolveAndInstallPnpmVersion>>
  try {
    result = await resolveAndInstallPnpmVersion(versionToInstall, {
      envLockfile: existingEnvLockfile,
      rootDir: context.rootProjectManifestDir,
      registries: config.registries,
      storeController: storeToUse.ctrl,
      storeDir: storeToUse.dir,
      virtualStoreDirMaxLength: config.virtualStoreDirMaxLength,
      packageManager: { name: packageManager.name, version: packageManager.version },
    })
  } finally {
    await storeToUse.ctrl.close()
  }

  if (!result.resolvedVersion) {
    globalWarn(`Cannot resolve pnpm version for "${pm.version}"`)
    return
  }
  const pmVersion = result.resolvedVersion

  // If the wanted version matches the current version, no switch needed.
  if (pmVersion === packageManager.version) return

  const wantedPnpmBinDir = result.binDir
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
