import path from 'path'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import exists from 'path-exists'
import runLifecycleHook, { RunLifecycleHookOptions } from './runLifecycleHook'
import runLifecycleHooksConcurrently, { RunLifecycleHooksConcurrentlyOptions } from './runLifecycleHooksConcurrently'

export function makeNodeRequireOption (modulePath: string) {
  let { NODE_OPTIONS } = process.env
  NODE_OPTIONS = `${NODE_OPTIONS ?? ''} --require=${modulePath}`.trim()
  return { NODE_OPTIONS }
}

export default runLifecycleHook
export {
  runLifecycleHooksConcurrently,
  RunLifecycleHookOptions,
  RunLifecycleHooksConcurrentlyOptions,
}

export async function runPostinstallHooks (
  opts: RunLifecycleHookOptions
): Promise<boolean> {
  const pkg = await readPackageJsonFromDir(opts.pkgRoot)
  if (pkg.scripts == null) {
    pkg.scripts = {}
  }

  if (!pkg.scripts.install) {
    await checkBindingGyp(opts.pkgRoot, pkg.scripts)
  }

  if (pkg.scripts.preinstall) {
    await runLifecycleHook('preinstall', pkg, opts)
  }
  if (pkg.scripts.install) {
    await runLifecycleHook('install', pkg, opts)
  }
  if (pkg.scripts.postinstall) {
    await runLifecycleHook('postinstall', pkg, opts)
  }

  return pkg.scripts.preinstall != null ||
    pkg.scripts.install != null ||
    pkg.scripts.postinstall != null
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
