import fs from 'node:fs'
import path from 'node:path'

import * as find from 'empathic/find'

/**
 * The names `process.argv[1]` takes when pnpm itself is the entry. Both
 * spellings occur because `process.argv[1]` keeps the name the process was
 * launched through: npm links its bins as symlinks, so it is `pn` or `pnpm`,
 * while pnpm's own shims and Corepack name the target, so it is `pnpm.mjs` or
 * `pnpm.cjs`.
 */
const PNPM_ENTRY_SCRIPTS = new Set(['pnpm', 'pn', 'pnpm.cjs', 'pnpm.mjs'])

/**
 * The names of the entries that run pnpm as `pnpm dlx`. Each prepends `dlx` to
 * `process.argv` and then loads the `pnpm.mjs` beside it, so re-running one
 * would turn `add` into `dlx add`, while that `pnpm.mjs` is pnpm's own entry.
 */
const PNPX_ENTRY_SCRIPTS = new Set(['pnpx', 'pnx', 'pnpx.cjs', 'pnpx.mjs'])

/**
 * The entry script that starts the pnpm whose code is running as
 * `selfModule`, found from `entryScript`, the script this process was launched
 * through. `undefined` when `entryScript` does not start that pnpm.
 *
 * pnpm's own code knows where it lives, so an entry is accepted only when its
 * real path is that module, as with a bundle run directly, or lies in the same
 * package, as with `bin/pnpm.mjs` loading `dist/pnpm.mjs`. Anything else,
 * including a host that merely imports pnpm's packages, such as Jest, and an
 * entry that cannot be resolved, is not pnpm as far as anything here can tell.
 * A `pnpx` entry yields the `pnpm.mjs` beside it.
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
 * Whether `execPath` is one of the `pnpx` and `pnx` aliases of the `@pnpm/exe`
 * executable. On Windows those are hardlinks of the executable, so the name it
 * was launched through is the only sign that it runs as `pnpm dlx`.
 */
export function isPnpxExecutable (execPath: string): boolean {
  const name = path.basename(execPath, path.extname(execPath)).toLowerCase()
  return name === 'pnpx' || name === 'pnx'
}

/**
 * The `@pnpm/exe` executable that re-invokes itself as pnpm: `execPath`, or
 * the `pnpm` executable linked beside a `pnpx` or `pnx` alias.
 */
export function findPnpmExecutable (execPath: string): string {
  if (!isPnpxExecutable(execPath)) return execPath
  return path.join(path.dirname(execPath), `pnpm${path.extname(execPath)}`)
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
