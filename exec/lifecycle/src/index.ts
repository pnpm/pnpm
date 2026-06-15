import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'

import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook.js'
import { runLifecycleHooksConcurrently, type RunLifecycleHooksConcurrentlyOptions } from './runLifecycleHooksConcurrently.js'

export function makeNodeRequireOption (modulePath: string): { NODE_OPTIONS: string } {
  let { NODE_OPTIONS } = process.env
  NODE_OPTIONS = `${NODE_OPTIONS ?? ''} --require=${modulePath}`.trim()
  return { NODE_OPTIONS }
}

export function makeNodePackageMapOption (packageMapPath: string, env?: Record<string, string | undefined>): { NODE_OPTIONS: string } {
  let { NODE_OPTIONS } = env ?? process.env
  NODE_OPTIONS = `${NODE_OPTIONS ?? process.env.NODE_OPTIONS ?? ''} --experimental-package-map=${quotePathIfNeeded(packageMapPath)}`.trim()
  return { NODE_OPTIONS }
}

function quotePathIfNeeded (path: string): string {
  return /\s/.test(path) ? JSON.stringify(path) : path
}

export {
  runLifecycleHook,
  type RunLifecycleHookOptions,
  runLifecycleHooksConcurrently,
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
