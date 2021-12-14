import path from 'path'
import PnpmError from '@pnpm/error'
import { safeReadPackageFromDir } from '@pnpm/read-package-json'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import preferredPM from 'preferred-pm'

export default async function preparePackage (pkgDir: string) {
  const manifest = await safeReadPackageFromDir(pkgDir)
  if (manifest?.scripts?.prepare != null && manifest.scripts.prepare !== '') {
    const pm = (await preferredPM(pkgDir))?.name ?? 'npm'
    try {
      await execa(pm, ['install'], { cwd: pkgDir })
    } catch (err: any) { // eslint-disable-line
      throw new PnpmError('PREPARE_PKG_FAILURE', err.shortMessage ?? err.message)
    }
    await rimraf(path.join(pkgDir, 'node_modules'))
  }
}
