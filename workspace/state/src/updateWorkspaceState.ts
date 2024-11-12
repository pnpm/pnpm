import fs from 'fs'
import path from 'path'
import { type Catalogs } from '@pnpm/catalogs.types'
import { logger } from '@pnpm/logger'
import { getFilePath } from './filePath'
import { createWorkspaceState } from './createWorkspaceState'
import { type ProjectsList } from './types'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  catalogs: Catalogs | undefined
  lastValidatedTimestamp: number
  workspaceDir: string
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState(opts)
  const workspaceStateJSON = JSON.stringify(workspaceState, undefined, 2) + '\n'
  const cacheFile = getFilePath(opts.workspaceDir)
  await fs.promises.mkdir(path.dirname(cacheFile), { recursive: true })
  await fs.promises.writeFile(cacheFile, workspaceStateJSON)
}
