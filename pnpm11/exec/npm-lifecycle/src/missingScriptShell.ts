import fs from 'node:fs'

import { PnpmError } from '@pnpm/error'

import type { SpawnError } from './spawn.js'

export const SCRIPT_SHELL_NOT_FOUND = 'ERR_PNPM_SCRIPT_SHELL_NOT_FOUND'

/**
 * Names the configured `scriptShell` when it is what could not be spawned.
 *
 * A missing working directory fails the spawn with the same `ENOENT`, so the
 * shell is blamed only when `wd` exists. A shell exiting with 127 is reported
 * as `ENOENT` too, but with a bare `spawn` syscall.
 */
export function missingScriptShellError (err: SpawnError, scriptShell: string | undefined, wd: string): PnpmError | undefined {
  if (!scriptShell || err.code !== 'ENOENT' || err.syscall !== `spawn ${scriptShell}`) return undefined
  if (!isDirectory(wd)) return undefined
  return new PnpmError('SCRIPT_SHELL_NOT_FOUND', `The configured scriptShell was not found: ${scriptShell}`, {
    hint: 'Set scriptShell in pnpm-workspace.yaml to the path of an existing shell executable, or unset it to use the default shell.',
  })
}

function isDirectory (dir: string): boolean {
  try {
    return fs.statSync(dir).isDirectory()
  } catch {
    return false
  }
}
