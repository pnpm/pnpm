import fs from 'fs'
import path from 'path'
import { parsePackageManager } from '@pnpm/cli-utils'
import { type Config } from '@pnpm/config'
import { detectIfCurrentPkgIsExecutable, packageManager } from '@pnpm/cli-meta'
import { prependDirsToPath } from '@pnpm/env.path'
import spawn from 'cross-spawn'
import { pnpmCmds } from './cmd'

export async function switchCliVersion (packageManagerFieldValue: string, config: Config): Promise<void> {
  const pm = parsePackageManager(packageManagerFieldValue)
  if (pm.name !== 'pnpm' || pm.version == null || pm.version === packageManager.version) return
  const pkgName = detectIfCurrentPkgIsExecutable() ? '@pnpm/exe' : 'pnpm'
  const dir = path.join(config.pnpmHomeDir, '.tools', pkgName, pm.version)
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
