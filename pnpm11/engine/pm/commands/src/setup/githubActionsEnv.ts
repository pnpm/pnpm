// Persist PNPM_HOME and $PNPM_HOME/bin for the rest of a GitHub Actions job.
//
// Every workflow step gets a fresh shell, so the rc-file edit addDirToEnvPath
// makes is invisible to the steps that follow. The runner instead reads back
// two line-oriented files, named by GITHUB_ENV and GITHUB_PATH, and applies
// each record to the rest of the job.
//
// Both files belong to the runner, which creates them up front. A path that
// holds anything else is not the runner's target. A missing file, a symlink,
// or a directory is therefore left alone rather than created or followed.

import fs from 'node:fs'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'

/**
 * Called before `setup` performs any side effect, so an unusable value aborts
 * the command instead of half-completing it.
 */
export function validateValues (pnpmHomeDir: string, binDir: string): void {
  if (!shouldPersist()) return
  validateValue('PNPM_HOME', pnpmHomeDir)
  validateValue('pnpm setup bin directory', binDir)
}

/**
 * A target that cannot be written is reported as a warning rather than
 * failing the command. The shell config is already updated by this point,
 * so the setup itself succeeded.
 */
export function persist (pnpmHomeDir: string, binDir: string): void {
  if (!shouldPersist()) return
  const githubEnv = process.env.GITHUB_ENV
  const githubPath = process.env.GITHUB_PATH
  if (githubEnv != null) {
    appendFile('GITHUB_ENV', githubEnv, `PNPM_HOME=${pnpmHomeDir}`)
  }
  if (githubPath != null) {
    appendFile('GITHUB_PATH', githubPath, binDir)
  }
}

function shouldPersist (): boolean {
  return process.env.GITHUB_ACTIONS === 'true' && (process.env.GITHUB_ENV != null || process.env.GITHUB_PATH != null)
}

/**
 * The files are line-oriented, so a line break in a persisted value would
 * append attacker-chosen records to the environment of every later step.
 */
function validateValue (name: string, value: string): void {
  if (value.includes('\n') || value.includes('\r') || value.includes('\0')) {
    throw new PnpmError('BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE', `${name} cannot contain newline or NUL characters`)
  }
}

function appendFile (targetName: string, filePath: string, line: string): void {
  try {
    if (!fs.lstatSync(filePath).isFile()) return
    appendLine(filePath, line)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') return
    logger.warn({
      message: `Failed to write GitHub Actions environment file ${targetName} (${filePath}): ${util.types.isNativeError(err) ? err.message : String(err)}`,
      prefix: process.cwd(),
    })
  }
}

function appendLine (filePath: string, line: string): void {
  const fd = fs.openSync(
    filePath,
    fs.constants.O_RDWR |
      fs.constants.O_APPEND |
      (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)
  )
  try {
    const stats = fs.fstatSync(fd)
    // The lstat above races with anything that swaps the path between the
    // two syscalls, so re-check through the descriptor.
    if (!stats.isFile()) return
    fs.writeSync(fd, `${leadingSeparator(fd, stats.size)}${line}\n`, null, 'utf8')
  } finally {
    fs.closeSync(fd)
  }
}

function leadingSeparator (fd: number, size: number): string {
  if (size === 0) return ''
  const lastByte = Buffer.allocUnsafe(1)
  const bytesRead = fs.readSync(fd, lastByte, 0, 1, size - 1)
  return bytesRead === 1 && lastByte[0] !== 0x0A ? '\n' : ''
}
