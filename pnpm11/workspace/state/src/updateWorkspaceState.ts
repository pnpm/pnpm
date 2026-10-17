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
import { loadWorkspaceState } from './loadWorkspaceState.js'
import type { ProjectsList, WorkspaceState, WorkspaceStateSettings } from './types.js'
import { recordValidation, trustsEveryFile } from './validation.js'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  settings: WorkspaceStateSettings
  workspaceDir: string
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: ConfigDependencies
  /**
   * Records which projects this write proves installed. Left out by a writer
   * that cannot tell, which records nothing.
   */
  validation?: StateValidationOptions
}

export interface StateValidationOptions {
  lockfileDir: string
  wantedLockfileName: string
  /** The patch files and the pnpmfiles every project shares. */
  sharedInputs: string[]
  /**
   * The root directories of the projects this write vouches for, or
   * `undefined` for every project. The others keep their recorded state.
   */
  vouchedProjects?: string[]
  /**
   * Write only a record the next check trusts every file of. A refresh that
   * only re-records unchanged files would otherwise be repeated by the next
   * check, which reads them again anyway.
   */
  onlyIfTrusted?: boolean
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState(opts)
  const cacheFile = getFilePath(opts.workspaceDir)
  const cacheDir = path.dirname(cacheFile)
  const tempFile = pathTemp(cacheDir)
  try {
    if (opts.validation != null) {
      recordStateValidation(workspaceState, opts, opts.validation)
      if (opts.validation.onlyIfTrusted && !(workspaceState.validation != null && trustsEveryFile(workspaceState.validation))) return
    }
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

/**
 * Records in `workspaceState` what `validation` proves. A record that cannot
 * be made leaves the state without one, which the next check proves from the
 * lockfiles.
 */
function recordStateValidation (
  workspaceState: WorkspaceState,
  opts: UpdateWorkspaceStateOptions,
  { vouchedProjects, onlyIfTrusted: _onlyIfTrusted, ...validation }: StateValidationOptions
): void {
  try {
    recordValidation(workspaceState, {
      ...validation,
      allProjects: opts.allProjects,
      workspaceDir: opts.workspaceDir,
      vouchedProjects: vouchedProjects == null ? undefined : new Set(vouchedProjects),
      previous: vouchedProjects == null ? undefined : loadWorkspaceState(opts.workspaceDir),
    })
  } catch (err: unknown) {
    delete workspaceState.validation
    globalWarn(`Failed to record the validation in the workspace state: ${isError(err) ? err.message : String(err)}`)
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
