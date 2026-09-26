import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { renameFileWithRetryAsync } from '@pnpm/fs.graceful-fs'
import { globalWarn, logger } from '@pnpm/logger'
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
  const tempFile = pathTemp(cacheDir)
  try {
    await fs.promises.mkdir(cacheDir, { recursive: true })
    await fs.promises.writeFile(tempFile, workspaceStateJSON)
    await renameFileWithRetryAsync(tempFile, cacheFile)
  } catch (err: unknown) {
    await fs.promises.rm(tempFile, { force: true }).catch(() => {})
    globalWarn(`Failed to write the workspace state: ${util.types.isNativeError(err) ? err.message : String(err)}`)
  }
}
