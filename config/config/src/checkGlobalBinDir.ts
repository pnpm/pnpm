import path from 'node:path'
import { promises as fs } from 'node:fs'

import PATH from 'path-name'
import { sync as canWriteToDir } from 'can-write-to-dir'

import { PnpmError } from '@pnpm/error'

export async function checkGlobalBinDir(
  globalBinDir: string,
  {
    env,
    shouldAllowWrite,
  }: { env: Record<string, string | undefined>; shouldAllowWrite?: boolean }
): Promise<void> {
  if (!env[PATH]) {
    throw new PnpmError(
      'NO_PATH_ENV',
      `Couldn't find a global directory for executables because the "${PATH}" environment variable is not set.`
    )
  }

  if (!(await globalBinDirIsInPath(globalBinDir, env))) {
    throw new PnpmError(
      'GLOBAL_BIN_DIR_NOT_IN_PATH',
      `The configured global bin directory "${globalBinDir}" is not in PATH`
    )
  }

  if (shouldAllowWrite && !canWriteToDirAndExists(globalBinDir)) {
    throw new PnpmError(
      'PNPM_DIR_NOT_WRITABLE',
      `The CLI has no write access to the pnpm home directory at ${globalBinDir}`
    )
  }
}

async function globalBinDirIsInPath(
  globalBinDir: string,
  env: Record<string, string | undefined>
): Promise<boolean> {
  const dirs = env[PATH]?.split(path.delimiter) ?? []
  if (dirs.some((dir) => areDirsEqual(globalBinDir, dir))) {
    return true
  }

  const realGlobalBinDir = await fs.realpath(globalBinDir)

  return dirs.some((dir) => areDirsEqual(realGlobalBinDir, dir))
}

const areDirsEqual = (dir1: string, dir2: string) =>
  path.relative(dir1, dir2) === ''

function canWriteToDirAndExists(dir: string): boolean {
  try {
    return canWriteToDir(dir)
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code !== 'ENOENT') throw err
    return false
  }
}
