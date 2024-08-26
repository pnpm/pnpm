import fs from 'fs'
import path from 'path'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { getCurrentPackageName, packageManager } from '@pnpm/cli-meta'
import { prependDirsToPath } from '@pnpm/env.path'
import { getToolDirPath } from '@pnpm/tools.path'
import spawn from 'cross-spawn'
import semver from 'semver'
import { pnpmCmds } from './cmd'

export async function switchCliVersion (config: Config): Promise<void> {
  const pm = config.wantedPackageManager
  if (pm == null || pm.name !== 'pnpm' || pm.version == null || pm.version === packageManager.version) return
  if (!semver.valid(pm.version)) {
    globalWarn(`Cannot switch to pnpm@${pm.version}: "${pm.version}" is not a valid version`)
    return
  }
  const pkgName = getCurrentPackageName()
  const dir = getToolDirPath({
    pnpmHomeDir: config.pnpmHomeDir,
    tool: {
      name: pkgName,
      version: pm.version,
    },
  })
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true })
    fs.writeFileSync(path.join(dir, 'package.json'), '{}')
    await pnpmCmds.add(
      {
        ...config,
        dir,
        lockfileDir: dir,
        bin: path.join(dir, 'bin'),
      },
      [`${pkgName}@${pm.version}`]
    )
  }
  const wantedPnpmBinDir = path.join(dir, 'bin')
  const pnpmEnv = prependDirsToPath([wantedPnpmBinDir])
  if (!pnpmEnv.updated) {
    // We throw this error to prevent an infinite recursive call of the same pnpm version.
    throw new PnpmError('VERSION_SWITCH_FAIL', `Failed to switch pnpm to v${pm.version}. Looks like pnpm CLI is missing at "${wantedPnpmBinDir}" or is incorrect`)
  }
  const { status } = spawn.sync('pnpm', process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  process.exit(status ?? 0)
}
