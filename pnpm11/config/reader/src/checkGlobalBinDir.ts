import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { canWriteToDirSync } from 'can-write-to-dir'
import PATH from 'path-name'

export async function checkGlobalBinDir (
  globalBinDir: string,
  { env, shouldAllowWrite }: { env: Record<string, string | undefined>, shouldAllowWrite?: boolean }
): Promise<void> {
  const pathEnv = env[PATH]
  if (!pathEnv) {
    throw new PnpmError('NO_PATH_ENV',
      `Couldn't find a global directory for executables because the "${PATH}" environment variable is not set.`)
  }
  if (!await globalBinDirIsInPath(globalBinDir, env)) {
    throw new PnpmError('GLOBAL_BIN_DIR_NOT_IN_PATH', `The configured global bin directory "${globalBinDir}" is not in PATH`, {
      hint: notInPathHint(pathEnv),
    })
  }
  if (shouldAllowWrite && !canWriteToDirAndExists(globalBinDir)) {
    throw new PnpmError('PNPM_DIR_NOT_WRITABLE', `The CLI has no write access to the global bin directory at ${globalBinDir}`)
  }
}

/**
 * An entry such as `%PNPM_HOME%\bin` reaching the process verbatim means
 * Windows did not expand the variable it references: the variable is unset,
 * or it is a user variable stored as REG_EXPAND_SZ, which Windows does not
 * expand inside the user Path. Name that entry instead of suggesting a PATH
 * change the user has seemingly already made. Elsewhere `%` is not expansion
 * syntax, so the entry is taken literally.
 */
function notInPathHint (pathEnv: string): string {
  const unexpanded = process.platform === 'win32'
    ? pathEnv.split(path.delimiter).find(hasUnexpandedEnvReference)
    : undefined
  if (unexpanded == null) return 'Run "pnpm setup" to update your shell configuration.'
  return `PATH contains "${unexpanded}", which was not expanded. A variable referenced from the user Path must be set to a full path, without references such as %LOCALAPPDATA%, and stored as a plain string (REG_SZ), not an expandable string (REG_EXPAND_SZ). Fix the variable, then open a new terminal.`
}

function hasUnexpandedEnvReference (dir: string): boolean {
  // Segments at odd indexes sit between a pair of `%` when another segment
  // follows them.
  const segments = dir.split('%')
  for (let index = 1; index < segments.length - 1; index += 2) {
    if (segments[index] !== '') return true
  }
  return false
}

async function globalBinDirIsInPath (globalBinDir: string, env: Record<string, string | undefined>): Promise<boolean> {
  const dirs = env[PATH]?.split(path.delimiter) ?? []
  if (dirs.some((dir) => areDirsEqual(globalBinDir, dir))) return true
  const realGlobalBinDir = await fs.realpath(globalBinDir)
  return dirs.some((dir) => areDirsEqual(realGlobalBinDir, dir))
}

const areDirsEqual = (dir1: string, dir2: string): boolean =>
  path.relative(dir1, dir2) === ''

function canWriteToDirAndExists (dir: string): boolean {
  try {
    return canWriteToDirSync(dir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}
