import fs from 'node:fs'
import path from 'node:path'

import * as find from 'empathic/find'

/**
 * The names `process.argv[1]` takes when pnpm itself is the entry. Both
 * spellings occur because `process.argv[1]` keeps the name the process was
 * launched through: npm links its bins as symlinks, so it is `pn` or `pnpm`,
 * while pnpm's own shims and Corepack name the target, so it is `pnpm.mjs` or
 * `pnpm.cjs`.
 *
 * `pnpx` and `pnx` are absent on purpose: they belong to pnpm's package too,
 * but that entry rewrites `process.argv` to prepend `dlx`, so re-running it
 * would turn `add` into `dlx add`.
 */
const PNPM_ENTRY_SCRIPTS = new Set(['pnpm', 'pn', 'pnpm.cjs', 'pnpm.mjs'])

/**
 * Returns `entryScript` when it starts the pnpm whose code is running as
 * `selfModule`, and `undefined` otherwise.
 *
 * pnpm's own code knows where it lives, so `entryScript` is accepted only when
 * its real path is that module, as with a bundle run directly, or lies in the
 * same package, as with `bin/pnpm.mjs` loading `dist/pnpm.mjs`. Anything else,
 * including a host that merely imports pnpm's packages, such as Jest, and an
 * entry that cannot be resolved, is not pnpm as far as anything here can tell.
 */
export function findPnpmEntryScript (entryScript: string | undefined, selfModule: string): string | undefined {
  if (entryScript == null || !PNPM_ENTRY_SCRIPTS.has(path.basename(entryScript).toLowerCase())) return undefined
  const entryPath = realpathOrUndefined(entryScript)
  const selfPath = realpathOrUndefined(selfModule)
  if (entryPath == null || selfPath == null) return undefined
  if (entryPath === selfPath) return entryScript
  const selfManifest = findOwningManifest(selfPath)
  return selfManifest != null && selfManifest === findOwningManifest(entryPath) ? entryScript : undefined
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
