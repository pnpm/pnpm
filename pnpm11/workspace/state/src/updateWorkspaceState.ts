import fs from 'node:fs'
import path from 'node:path'

import { renameFileWithRetry } from '@pnpm/fs.graceful-fs'
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
  try {
    await fs.promises.mkdir(cacheDir, { recursive: true })
    const tmp = pathTemp(cacheDir)
    try {
      await fs.promises.writeFile(tmp, workspaceStateJSON)
      renameFileWithRetry(tmp, cacheFile)
    } catch (err) {
      await fs.promises.unlink(tmp).catch(() => {})
      throw err
    }
  } catch (err: unknown) {
    globalWarn(`Failed to write the workspace state: ${err instanceof Error ? err.message : String(err)}`)
  }
}

export const updateWorkspaceStateOrWarn = updateWorkspaceState
