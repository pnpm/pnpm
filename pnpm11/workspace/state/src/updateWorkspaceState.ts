import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { renameFileWithRetryAsync } from '@pnpm/fs.graceful-fs'
import { globalWarn, logger } from '@pnpm/logger'
import type { ConfigDependencies } from '@pnpm/types'
import { pathTemp } from 'path-temp'

import { createWorkspaceState } from './createWorkspaceState.js'
import { getFilePath } from './filePath.js'
import { getHoistedProjectModulesDir } from './hoistedProjectModulesDir.js'
import type { ProjectsList, WorkspaceState, WorkspaceStateSettings } from './types.js'

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
  const cacheFile = getFilePath(opts.workspaceDir)
  const cacheDir = path.dirname(cacheFile)
  const tempFile = pathTemp(cacheDir)
  try {
    if (opts.settings.nodeLinker === 'hoisted') {
      await recordHoistedModulesDirs(workspaceState, opts.allProjects)
    }
    await fs.promises.mkdir(cacheDir, { recursive: true })
    await fs.promises.writeFile(tempFile, JSON.stringify(workspaceState, undefined, 2) + '\n')
    await renameFileWithRetryAsync(tempFile, cacheFile)
  } catch (err: unknown) {
    await fs.promises.rm(tempFile, { force: true }).catch(() => {})
    globalWarn(`Failed to write the workspace state: ${isError(err) ? err.message : String(err)}`)
  }
}

async function recordHoistedModulesDirs (workspaceState: WorkspaceState, allProjects: ProjectsList): Promise<void> {
  await Promise.all(allProjects.map(async (project) => {
    if (await isDirectory(getHoistedProjectModulesDir(project.rootDir))) {
      workspaceState.projects[project.rootDir].hasModulesDir = true
    }
  }))
}

async function isDirectory (dir: string): Promise<boolean> {
  try {
    return (await fs.promises.stat(dir)).isDirectory()
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return false
    throw err
  }
}
