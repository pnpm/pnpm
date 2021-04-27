import path from 'path'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import preferredPM from 'preferred-pm'

export default async function preparePackage (pkgDir: string) {
  const manifest = await readPackageJsonFromDir(pkgDir)
  if (manifest.scripts?.prepare != null && manifest.scripts.prepare !== '') {
    const pm = (await preferredPM(pkgDir))?.name ?? 'npm'
    await execa(pm, ['install'], { cwd: pkgDir })
    await rimraf(path.join(pkgDir, 'node_modules'))
  }
}
