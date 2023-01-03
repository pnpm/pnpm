import fs from 'fs'
import path from 'path'
import { runLifecycleHook, RunLifecycleHookOptions } from '@pnpm/lifecycle'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { PackageManifest } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import preferredPM from 'preferred-pm'
import omit from 'ramda/src/omit'

const PREPUBLISH_SCRIPTS = [
  'prepublish',
  'prepublishOnly',
  'prepack',
  'publish',
  'postpublish',
]

export interface PreparePackageOptions {
  rawConfig: object
  unsafePerm?: boolean
}

export async function preparePackage (opts: PreparePackageOptions, pkgDir: string) {
  const manifest = await safeReadPackageJsonFromDir(pkgDir)
  if (manifest?.scripts == null || !packageShouldBeBuilt(manifest, pkgDir)) return
  const pm = (await preferredPM(pkgDir))?.name ?? 'npm'
  const execOpts: RunLifecycleHookOptions = {
    depPath: `${manifest.name}@${manifest.version}`,
    pkgRoot: pkgDir,
    // We can't prepare a package without running its lifecycle scripts.
    // An alternative solution could be to throw an exception.
    rawConfig: omit(['ignore-scripts'], opts.rawConfig),
    rootModulesDir: pkgDir, // We don't need this property but there is currently no way to not set it.
    unsafePerm: Boolean(opts.unsafePerm),
  }
  try {
    const installScriptName = `${pm}-install`
    manifest.scripts[installScriptName] = `${pm} install`
    await runLifecycleHook(installScriptName, manifest, execOpts)
    for (const scriptName of PREPUBLISH_SCRIPTS) {
      if (manifest.scripts[scriptName] == null || manifest.scripts[scriptName] === '') continue
      await runLifecycleHook(scriptName, manifest, execOpts)
    }
  } catch (err: any) { // eslint-disable-line
    err.code = 'ERR_PNPM_PREPARE_PACKAGE'
    throw err
  }
  await rimraf(path.join(pkgDir, 'node_modules'))
}

function packageShouldBeBuilt (manifest: PackageManifest, pkgDir: string): boolean {
  if (manifest.scripts == null) return false
  const scripts = manifest.scripts
  if (scripts.prepare != null && scripts.prepare !== '') return true
  const hasPrepublishScript = PREPUBLISH_SCRIPTS.some((scriptName) => scripts[scriptName] != null && scripts[scriptName] !== '')
  if (!hasPrepublishScript) return false
  const mainFile = manifest.main ?? 'index.js'
  return !fs.existsSync(path.join(pkgDir, mainFile))
}
