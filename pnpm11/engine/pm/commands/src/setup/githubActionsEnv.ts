import fs from 'node:fs'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'

// Persist PNPM_HOME and $PNPM_HOME/bin for the rest of a GitHub Actions job.
// A workflow step gets a fresh shell, so the rc-file edit addDirToEnvPath
// makes is invisible to every later step. The runner instead reads back two
// line-oriented files whose paths arrive in GITHUB_ENV and GITHUB_PATH, and
// applies each record to the steps that follow.

/**
 * Reject a value that cannot be persisted, before `setup` performs any side
 * effect.
 */
export function validateGitHubActionsEnvironmentFileValues (pnpmHomeDir: string, binDir: string): void {
  if (!shouldPersistGitHubActionsEnvironmentFiles()) return
  validateGitHubActionsEnvironmentFileValue('PNPM_HOME', pnpmHomeDir)
  validateGitHubActionsEnvironmentFileValue('pnpm setup bin directory', binDir)
}

/**
 * `GITHUB_ENV` and `GITHUB_PATH` are line-oriented, so a line break in a
 * persisted value would append attacker-chosen records to the environment of
 * every later step in the workflow job.
 */
function validateGitHubActionsEnvironmentFileValue (name: string, value: string): void {
  if (value.includes('\n') || value.includes('\r') || value.includes('\0')) {
    throw new PnpmError('BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE', `${name} cannot contain newline or NUL characters`)
  }
}

/**
 * Append `PNPM_HOME` to `GITHUB_ENV` and the bin directory to `GITHUB_PATH`.
 * A target that cannot be written is reported as a warning and never stops the
 * other one from being written.
 */
export function writeGitHubActionsEnvironmentFiles (pnpmHomeDir: string, binDir: string): void {
  if (!shouldPersistGitHubActionsEnvironmentFiles()) return
  const githubEnv = process.env.GITHUB_ENV
  const githubPath = process.env.GITHUB_PATH
  if (githubEnv != null) {
    appendGitHubActionsEnvironmentFile('GITHUB_ENV', githubEnv, `PNPM_HOME=${pnpmHomeDir}`)
  }
  if (githubPath != null) {
    appendGitHubActionsEnvironmentFile('GITHUB_PATH', githubPath, binDir)
  }
}

function shouldPersistGitHubActionsEnvironmentFiles (): boolean {
  return process.env.GITHUB_ACTIONS === 'true' && (process.env.GITHUB_ENV != null || process.env.GITHUB_PATH != null)
}

function appendGitHubActionsEnvironmentFile (targetName: string, filePath: string, line: string): void {
  try {
    if (!fs.lstatSync(filePath).isFile()) return
    appendLineToRegularFile(filePath, line)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') return
    logger.warn({
      message: `Failed to write GitHub Actions environment file ${targetName} (${filePath}): ${util.types.isNativeError(err) ? err.message : String(err)}`,
      prefix: process.cwd(),
    })
  }
}

/**
 * The runner creates both files up front, so anything but an existing regular
 * file at `filePath` is not the runner's target: skip it instead of creating
 * it or following a symlink to it.
 */
function appendLineToRegularFile (filePath: string, line: string): void {
  const fd = fs.openSync(
    filePath,
    fs.constants.O_RDWR |
      fs.constants.O_APPEND |
      (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)
  )
  try {
    const stats = fs.fstatSync(fd)
    if (!stats.isFile()) return
    fs.writeSync(fd, `${missingRecordSeparator(fd, stats.size)}${line}\n`, null, 'utf8')
  } finally {
    fs.closeSync(fd)
  }
}

/** Start a record of its own even when the runner left the file without a trailing newline. */
function missingRecordSeparator (fd: number, size: number): string {
  if (size === 0) return ''
  const lastByte = Buffer.allocUnsafe(1)
  const bytesRead = fs.readSync(fd, lastByte, 0, 1, size - 1)
  return bytesRead === 1 && lastByte[0] !== 0x0A ? '\n' : ''
}
