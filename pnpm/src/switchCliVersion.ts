import fs from 'fs'
import path from 'path'
import { type Config } from '@pnpm/config'
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
  const pnpmEnv = prependDirsToPath([path.join(dir, 'bin')])
  const { status } = spawn.sync('pnpm', process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  process.exit(status ?? 0)
}
