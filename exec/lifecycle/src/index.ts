import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook'
import { runLifecycleHooksConcurrently, type RunLifecycleHooksConcurrentlyOptions } from './runLifecycleHooksConcurrently'

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

  if (pkg.scripts.preinstall) {
    await runLifecycleHook('preinstall', pkg, opts)
  }
  const executedAnInstallScript = await runLifecycleHook('install', pkg, opts)
  if (pkg.scripts.postinstall) {
    await runLifecycleHook('postinstall', pkg, opts)
  }

  return pkg.scripts.preinstall != null ||
    executedAnInstallScript ||
    pkg.scripts.postinstall != null
}
