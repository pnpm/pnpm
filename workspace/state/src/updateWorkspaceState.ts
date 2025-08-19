import fs from 'fs'
import path from 'path'
import { logger } from '@pnpm/logger'
import { getFilePath } from './filePath.js'
import { createWorkspaceState } from './createWorkspaceState.js'
import { type WorkspaceStateSettings, type ProjectsList } from './types.js'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  settings: WorkspaceStateSettings
  workspaceDir: string
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: Record<string, string>
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState(opts)
  const workspaceStateJSON = JSON.stringify(workspaceState, undefined, 2) + '\n'
  const cacheFile = getFilePath(opts.workspaceDir)
  await fs.promises.mkdir(path.dirname(cacheFile), { recursive: true })
  await fs.promises.writeFile(cacheFile, workspaceStateJSON)
}
