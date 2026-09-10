import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { renameFileWithRetry } from '@pnpm/fs.graceful-fs'
import { logger } from '@pnpm/logger'
import type { ConfigDependencies } from '@pnpm/types'
import { pathTemp } from 'path-temp'

import { createWorkspaceState } from './createWorkspaceState.js'
import { getFilePath } from './filePath.js'
import type { ProjectsList, WorkspaceStateSettings } from './types.js'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  settings: WorkspaceStateSettings
  workspaceDir: string
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: ConfigDependencies
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState(opts)
  const workspaceStateJSON = JSON.stringify(workspaceState, undefined, 2) + '\n'
  const cacheFile = getFilePath(opts.workspaceDir)
  const cacheDir = path.dirname(cacheFile)
  await fs.promises.mkdir(cacheDir, { recursive: true })
  // Written through a temp sibling so a concurrent reader sees either the
  // whole old file or the whole new one, and renamed with the retry that
  // waits out whoever else holds the destination open on Windows.
  const tmp = pathTemp(cacheDir)
  try {
    await fs.promises.writeFile(tmp, workspaceStateJSON)
    renameFileWithRetry(tmp, cacheFile)
  } catch (err) {
    await fs.promises.unlink(tmp).catch(() => {})
    throw err
  }
}

/**
 * Records the workspace state, reporting a failed write as a warning.
 *
 * The state file is a cache: losing a write only costs the next command a
 * repeat of the content check, so a write failure must not fail a command
 * whose real work has already succeeded
 * (https://github.com/pnpm/pnpm/issues/14550).
 */
export async function updateWorkspaceStateOrWarn (opts: UpdateWorkspaceStateOptions): Promise<void> {
  try {
    await updateWorkspaceState(opts)
  } catch (err: unknown) {
    const reason = util.types.isNativeError(err) ? err.message : String(err)
    logger.warn({
      message: `Failed to update the workspace state: ${reason}`,
      prefix: opts.workspaceDir,
    })
  }
}
