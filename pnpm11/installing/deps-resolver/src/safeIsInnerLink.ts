import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { renameFileWithRetry } from '@pnpm/fs.graceful-fs'
import { logger } from '@pnpm/logger'
import { isInnerLink } from 'is-inner-link'
import { isSubdir } from 'is-subdir'

export async function safeIsInnerLink (
  projectModulesDir: string,
  depName: string,
  opts: {
    hideAlienModules: boolean
    projectDir: string
    virtualStoreDir: string
    globalVirtualStoreDir: string
  }
): Promise<true | string> {
  try {
    const link = await isInnerLink(projectModulesDir, depName)

    if (link.isInner) return true

    if (
      isSubdir(opts.virtualStoreDir, link.target) ||
      opts.globalVirtualStoreDir !== opts.virtualStoreDir && isSubdir(opts.globalVirtualStoreDir, link.target)
    ) {
      return true
    }

    return link.target as string
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return true

    if (opts.hideAlienModules) {
      logger.warn({
        message: `Moving ${depName} that was installed by a different package manager to "node_modules/.ignored"`,
        prefix: opts.projectDir,
      })
      await moveAlienModule(
        path.join(projectModulesDir, depName),
        path.join(projectModulesDir, '.ignored', depName)
      )
    }
    return true
  }
}

async function moveAlienModule (alienDir: string, ignoredDir: string): Promise<void> {
  await fs.promises.rm(ignoredDir, { recursive: true, force: true })
  await fs.promises.mkdir(path.dirname(ignoredDir), { recursive: true })
  try {
    renameFileWithRetry(alienDir, ignoredDir)
  } catch (err: unknown) {
    if (!isWindowsFileLockError(err)) throw err
    throw new PnpmError('MODULES_DIR_IN_USE', `Could not move "${alienDir}" to "${ignoredDir}" (${err.code})`, {
      cause: err,
      hint: 'A file in this directory is probably in use by another process, such as a dev server or an editor. Stop that process and try again.',
    })
  }
}

function isWindowsFileLockError (err: unknown): err is NodeJS.ErrnoException {
  return process.platform === 'win32' &&
    util.types.isNativeError(err) &&
    'code' in err &&
    (err.code === 'EPERM' || err.code === 'EACCES' || err.code === 'EBUSY')
}
