import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { createProjectModulesDirResolver, type ProjectModulesDirOptions } from '@pnpm/config.reader'
import { renameFileWithRetryAsync } from '@pnpm/fs.graceful-fs'
import { globalWarn, logger } from '@pnpm/logger'
import type { ConfigDependencies } from '@pnpm/types'
import { pathTemp } from 'path-temp'

import { createWorkspaceState } from './createWorkspaceState.js'
import { getFilePath } from './filePath.js'
import type { ProjectsList, WorkspaceState, WorkspaceStateSettings } from './types.js'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  settings: WorkspaceStateSettings
  workspaceDir: string
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: ConfigDependencies
  /** Resolves each project's own modules directory, as the install did. */
  projectModulesDirs: ProjectModulesDirOptions
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState(opts)
  if (opts.settings.nodeLinker === 'hoisted') {
    await recordHoistedModulesDirs(workspaceState, opts)
  }
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

async function recordHoistedModulesDirs (
  workspaceState: WorkspaceState,
  opts: Pick<UpdateWorkspaceStateOptions, 'allProjects' | 'projectModulesDirs'>
): Promise<void> {
  const modulesDirOf = createProjectModulesDirResolver(opts.projectModulesDirs)
  await Promise.all(opts.allProjects.map(async (project) => {
    const modulesDir = path.resolve(project.rootDir, modulesDirOf(project.manifest.name) ?? 'node_modules')
    if (await isDirectory(modulesDir)) {
      workspaceState.projects[project.rootDir].hasModulesDir = true
    }
  }))
}

async function isDirectory (dir: string): Promise<boolean> {
  try {
    return (await fs.promises.stat(dir)).isDirectory()
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return false
    throw err
  }
}
