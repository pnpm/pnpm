import path from 'path'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import exists from 'path-exists'
import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook'
import { runLifecycleHooksConcurrently, type RunLifecycleHooksConcurrentlyOptions } from './runLifecycleHooksConcurrently'
import { type PackageScripts } from '@pnpm/types'

export function makeNodeRequireOption (modulePath: string): { NODE_OPTIONS: string } {
  let { NODE_OPTIONS } = process.env
  NODE_OPTIONS = `${NODE_OPTIONS ?? ''} --require=${modulePath}`.trim()
  return { NODE_OPTIONS }
}

export {
  runLifecycleHook,
  runLifecycleHooksConcurrently,
  type RunLifecycleHookOptions,
  type RunLifecycleHooksConcurrentlyOptions,
}

export async function runPostinstallHooks (
  opts: RunLifecycleHookOptions
): Promise<boolean> {
  const pkg = await safeReadPackageJsonFromDir(opts.pkgRoot)
  if (pkg == null) return false
  if (pkg.scripts == null) {
    pkg.scripts = {}
  }

  if (!pkg.scripts.install && !pkg.scripts.preinstall) {
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
 * Run node-gyp when binding.gyp is available. Only do this when there are no
 * `install` and `preinstall` scripts (see `npm help scripts`).
 */
async function checkBindingGyp (
  root: string,
  scripts: PackageScripts
): Promise<void> {
  if (await exists(path.join(root, 'binding.gyp'))) {
    scripts.install = 'node-gyp rebuild'
  }
}
