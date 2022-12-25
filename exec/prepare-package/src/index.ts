import path from 'path'
import { PnpmError } from '@pnpm/error'
import lifecycle from '@pnpm/npm-lifecycle'
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

export async function preparePackage (pkgDir: string, opts: { rawConfig: object }) {
  const manifest = await safeReadPackageJsonFromDir(pkgDir)
  if (manifest?.scripts == null || !packageShouldBeBuilt(manifest.scripts)) return
  const pm = (await preferredPM(pkgDir))?.name ?? 'npm'
  const env = lifecycle.makeEnv(manifest, { config: opts.rawConfig })
  const execOpts = { cwd: pkgDir, env, extendEnv: true }
  try {
    await execa(pm, ['install'], execOpts)
    for (const scriptName of PREPUBLISH_SCRIPTS) {
      if (manifest.scripts[scriptName] == null || manifest.scripts[scriptName] === '') continue
      await execa(pm, ['run', scriptName], execOpts)
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
