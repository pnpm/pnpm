import path from 'path'
import { PnpmError } from '@pnpm/error'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { PackageScripts } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import preferredPM from 'preferred-pm'

const PREPARE_SCRIPTS = [
  'preinstall',
  'install',
  'postinstall',
  'prepare',
]

const PREPUBLISH_SCRIPTS = [
  'prepublish',
  'prepublishOnly',
  'prepack',
  'publish',
  'postpublish',
]

export async function preparePackage (pkgDir: string) {
  const manifest = await safeReadPackageJsonFromDir(pkgDir)
  if (manifest?.scripts == null || !packageShouldBeBuilt(manifest.scripts)) return
  const pm = (await preferredPM(pkgDir))?.name ?? 'npm'
  try {
    await execa(pm, ['install'], { cwd: pkgDir })
    for (const scriptName of PREPUBLISH_SCRIPTS) {
      if (manifest.scripts[scriptName] == null || manifest.scripts[scriptName] === '') continue
      await execa(pm, ['run', scriptName], { cwd: pkgDir })
    }
  } catch (err: any) { // eslint-disable-line
    throw new PnpmError('PREPARE_PKG_FAILURE', err.shortMessage ?? err.message)
  }
  await rimraf(path.join(pkgDir, 'node_modules'))
}

function packageShouldBeBuilt (packageScripts: PackageScripts): boolean {
  return [
    ...PREPUBLISH_SCRIPTS,
    ...PREPARE_SCRIPTS,
  ].some((scriptName) => packageScripts[scriptName] != null && packageScripts[scriptName] !== '')
}
