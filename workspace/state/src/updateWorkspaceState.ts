import fs from 'fs'
import path from 'path'
import { logger } from '@pnpm/logger'
import { getFilePath } from './filePath'
import { createWorkspaceState } from './createWorkspaceState'
import { type WorkspaceStateSettings, type ProjectsList } from './types'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  settings: WorkspaceStateSettings
  workspaceDir: string
  pnpmfileExists: boolean
  filteredInstall: boolean
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState(opts)
  const workspaceStateJSON = JSON.stringify(workspaceState, undefined, 2) + '\n'
  const cacheFile = getFilePath(opts.workspaceDir)
  await fs.promises.mkdir(path.dirname(cacheFile), { recursive: true })
  await fs.promises.writeFile(cacheFile, workspaceStateJSON)
}
