import path from 'path'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'

export default async function preparePackage (pkgDir: string) {
  const manifest = await readPackageJsonFromDir(pkgDir)
  if (manifest.scripts?.prepare != null && manifest.scripts.prepare !== '') {
    await execa('pnpm', ['install'], { cwd: pkgDir })
    await rimraf(path.join(pkgDir, 'node_modules'))
  }
}
