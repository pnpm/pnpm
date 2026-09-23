import fs from 'node:fs'
import path from 'node:path'

import * as find from 'empathic/find'

/**
 * `process.argv[1]` keeps the name the process was launched through: npm's bin
 * symlinks, or the target that pnpm's shims and Corepack name.
 */
const PNPM_ENTRY_SCRIPTS = new Set(['pnpm', 'pn', 'pnpm.cjs', 'pnpm.mjs'])

/**
 * These prepend `dlx` to `process.argv` before loading the `pnpm.mjs` beside
 * them, so that `pnpm.mjs`, not the `pnpx` entry, re-invokes pnpm.
 */
const PNPX_ENTRY_SCRIPTS = new Set(['pnpx', 'pnx', 'pnpx.cjs', 'pnpx.mjs'])

/**
 * The entry script that re-invokes the pnpm whose code is `selfModule`, given
 * `entryScript`, the script this process was launched through, or `undefined`
 * when that is not pnpm. An entry counts as pnpm only when its real path is
 * `selfModule` or shares its package, so a host that merely imports pnpm's
 * packages and an entry that cannot be resolved are both rejected.
 */
export function findPnpmEntryScript (entryScript: string | undefined, selfModule: string): string | undefined {
  if (entryScript == null) return undefined
  const entryName = path.basename(entryScript).toLowerCase()
  if (PNPM_ENTRY_SCRIPTS.has(entryName)) {
    return belongsToRunningPnpm(entryScript, selfModule) ? entryScript : undefined
  }
  if (PNPX_ENTRY_SCRIPTS.has(entryName)) {
    const pnpxPath = realpathOrUndefined(entryScript)
    if (pnpxPath == null) return undefined
    const pnpmEntry = path.join(path.dirname(pnpxPath), 'pnpm.mjs')
    return belongsToRunningPnpm(pnpmEntry, selfModule) ? pnpmEntry : undefined
  }
  return undefined
}

/**
 * The `@pnpm/exe` executable that re-invokes itself as pnpm: `execPath`, or
 * the `pnpm` executable linked beside a `pnpx` or `pnx` alias.
 */
export function findPnpmExecutable (execPath: string): string {
  if (!isPnpxExecutable(execPath)) return execPath
  return path.join(path.dirname(execPath), `pnpm${path.extname(execPath)}`)
}

/**
 * Whether `execPath` is one of the `pnpx` and `pnx` aliases of the `@pnpm/exe`
 * executable. On Windows those are hardlinks of the executable, so the name it
 * was launched through is the only sign that it runs as `pnpm dlx`.
 */
export function isPnpxExecutable (execPath: string): boolean {
  const name = path.basename(execPath, path.extname(execPath)).toLowerCase()
  return name === 'pnpx' || name === 'pnx'
}

function belongsToRunningPnpm (entryScript: string, selfModule: string): boolean {
  const entryPath = realpathOrUndefined(entryScript)
  const selfPath = realpathOrUndefined(selfModule)
  if (entryPath == null || selfPath == null) return false
  if (entryPath === selfPath) return true
  const selfManifest = findOwningManifest(selfPath)
  return selfManifest != null && selfManifest === findOwningManifest(entryPath)
}

function findOwningManifest (file: string): string | undefined {
  return find.file('package.json', { cwd: path.dirname(file) })
}

function realpathOrUndefined (file: string): string | undefined {
  try {
    return fs.realpathSync(file)
  } catch {
    return undefined
  }
}
