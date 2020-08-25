import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import runLifecycleHook, { RunLifecycleHookOptions } from './runLifecycleHook'
import runLifecycleHooksConcurrently from './runLifecycleHooksConcurrently'
import path = require('path')
import exists = require('path-exists')

export default runLifecycleHook
export { runLifecycleHooksConcurrently, RunLifecycleHookOptions }

export async function runPostinstallHooks (
  opts: {
    depPath: string
    extraBinPaths?: string[]
    optional?: boolean
    pkgRoot: string
    prepare?: boolean
    rawConfig: object
    rootModulesDir: string
    unsafePerm: boolean
  }
): Promise<boolean> {
  const pkg = await readPackageJsonFromDir(opts.pkgRoot)
  const scripts = pkg?.scripts ?? {}

  if (!scripts.install) {
    await checkBindingGyp(opts.pkgRoot, scripts)
  }

  if (scripts.preinstall) {
    await runLifecycleHook('preinstall', pkg, opts)
  }
  if (scripts.install) {
    await runLifecycleHook('install', pkg, opts)
  }
  if (scripts.postinstall) {
    await runLifecycleHook('postinstall', pkg, opts)
  }

  if (opts.prepare && scripts.prepare) {
    await runLifecycleHook('prepare', pkg, opts)
  }

  return !!scripts.preinstall || !!scripts.install || !!scripts.postinstall
}

/**
 * Run node-gyp when binding.gyp is available. Only do this when there's no
 * `install` script (see `npm help scripts`).
 */
async function checkBindingGyp (
  root: string,
  scripts: {}
) {
  if (await exists(path.join(root, 'binding.gyp'))) {
    scripts['install'] = 'node-gyp rebuild' // eslint-disable-line @typescript-eslint/dot-notation
  }
}
