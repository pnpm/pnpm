import util from 'node:util'

import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'

import { makeProjectNodePathOption } from './makeProjectNodePathOption.js'
import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook.js'
import {
  POST_UNINSTALL_STAGES,
  PRE_UNINSTALL_STAGES,
  PROJECT_INSTALL_STAGES,
  PROJECT_LIFECYCLE_STAGES,
  runLifecycleHooksConcurrently,
  type RunLifecycleHooksConcurrentlyOptions,
} from './runLifecycleHooksConcurrently.js'
import { killTrackedProcessTrees, type TrackableChildProcess, trackChildProcess } from './trackChildProcess.js'

/**
 * Whether `err` is the failure of a lifecycle script itself, as opposed to
 * an error raised while preparing or linking around it.
 */
export function isLifecycleScriptError (err: unknown): boolean {
  return util.types.isNativeError(err) && typeof (err as { stage?: unknown }).stage === 'string' && typeof (err as { script?: unknown }).script === 'string'
}

export function makeNodeRequireOption (modulePath: string, env?: Record<string, string | undefined>): { NODE_OPTIONS: string } {
  let { NODE_OPTIONS } = env ?? process.env
  NODE_OPTIONS = `${NODE_OPTIONS ?? process.env.NODE_OPTIONS ?? ''} --require=${quotePathIfNeeded(modulePath)}`.trim()
  return { NODE_OPTIONS }
}

export function makeNodePackageMapOption (packageMapPath: string, env?: Record<string, string | undefined>): { NODE_OPTIONS: string } {
  let { NODE_OPTIONS } = env ?? process.env
  NODE_OPTIONS = `${removeNodePackageMapOption(NODE_OPTIONS ?? process.env.NODE_OPTIONS ?? '')} --experimental-package-map=${quotePathIfNeeded(packageMapPath)}`.trim()
  return { NODE_OPTIONS }
}

// Node's NODE_OPTIONS tokenizer splits on whitespace, treats `'` and `"` as
// quote delimiters, and uses `\` as an escape character. A bare path with a
// space, quote, or backslash (e.g. any Windows path) would therefore be
// mis-parsed. Wrap such paths in double quotes and escape `\` and `"` so the
// tokenizer reconstructs the literal path.
function quotePathIfNeeded (path: string): string {
  if (!/[\s"'\\]/.test(path)) return path
  return `"${path.replace(/(["\\])/g, '\\$1')}"`
}

function removeNodePackageMapOption (nodeOptions: string): string {
  // The quoted-value patterns span backslash escapes (`\"`), matching the
  // escaping `makeNodePackageMapOption` emits, so an existing flag whose path
  // contains a quote is still stripped in full.
  return nodeOptions
    .replace(/(?:^|\s)--experimental-package-map=(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\S+)/g, '')
    .replace(/(?:^|\s)--experimental-package-map\s+(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\S+)/g, '')
    .trim()
}

export {
  killTrackedProcessTrees,
  makeProjectNodePathOption,
  POST_UNINSTALL_STAGES,
  PRE_UNINSTALL_STAGES,
  PROJECT_INSTALL_STAGES,
  PROJECT_LIFECYCLE_STAGES,
  runLifecycleHook,
  type RunLifecycleHookOptions,
  runLifecycleHooksConcurrently,
  type RunLifecycleHooksConcurrentlyOptions,
  type TrackableChildProcess,
  trackChildProcess,
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
