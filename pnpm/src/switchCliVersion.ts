import fs from 'fs'
import path from 'path'
import { type Config } from '@pnpm/config'
import { globalWarn } from '@pnpm/logger'
import { detectIfCurrentPkgIsExecutable, packageManager } from '@pnpm/cli-meta'
import { prependDirsToPath } from '@pnpm/env.path'
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
  const pkgName = detectIfCurrentPkgIsExecutable() ? getExePackageName() : 'pnpm'
  const dir = path.join(config.pnpmHomeDir, '.tools', pkgName.replaceAll('/', '+'), pm.version)
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

function getExePackageName () {
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch

  return `@pnpm/${platform}-${arch}`
}
