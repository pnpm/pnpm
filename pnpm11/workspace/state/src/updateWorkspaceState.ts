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
import { type ReadLocalDependencies, recordLocalDependencies } from './localDependencies.js'
import { resolveStoreLocationSettings } from './storeLocation.js'
import type { ProjectsList, WorkspaceState, WorkspaceStateSettings } from './types.js'
import { type RecordedValidation, recordValidation, trustsEveryFile } from './validation.js'

export interface UpdateWorkspaceStateOptions {
  allProjects: ProjectsList
  settings: WorkspaceStateSettings
  workspaceDir: string
  /** The directory a relative `storeDir` resolves against. Defaults to `workspaceDir`. */
  storeBaseDir?: string
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: ConfigDependencies
  /**
   * Settings this caller did not compare and keeps from the previous state,
   * such as `dev`/`optional`/`production` during a `run` or `exec` refresh.
   */
  ignoredWorkspaceStateSettings?: Array<keyof WorkspaceStateSettings>
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
  sharedWorkspaceLockfile?: boolean
  /**
   * Reads the local dependencies of a lockfile whose digest the previous
   * state does not record them for. Left out by a writer that holds no
   * lockfile reader, which then records none for such a lockfile.
   */
  readLocalDependencies?: ReadLocalDependencies
}

export async function updateWorkspaceState (opts: UpdateWorkspaceStateOptions): Promise<void> {
  logger.debug({ msg: 'updating workspace state' })
  const workspaceState = createWorkspaceState({
    ...opts,
    settings: { ...opts.settings, ...resolveStoreLocationSettings(opts.settings, {
      stateDir: opts.workspaceDir,
      storeBaseDir: opts.storeBaseDir ?? opts.workspaceDir,
    }) },
  })
  const cacheFile = getFilePath(opts.workspaceDir)
  const cacheDir = path.dirname(cacheFile)
  const tempFile = pathTemp(cacheDir)
  try {
    const previous = carriesPreviousState(opts) ? readPreviousWorkspaceState(opts.workspaceDir) : undefined
    preserveCarriedSettings(workspaceState.settings, previous?.settings, opts.ignoredWorkspaceStateSettings)
    if (opts.validation != null && !(await recordValidationToWrite(workspaceState, opts, opts.validation, previous))) return
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

function carriesPreviousState (opts: UpdateWorkspaceStateOptions): boolean {
  return opts.ignoredWorkspaceStateSettings != null || opts.validation != null
}

function readPreviousWorkspaceState (workspaceDir: string): WorkspaceState | undefined {
  try {
    return loadWorkspaceState(workspaceDir)
  } catch (err: unknown) {
    globalWarn(`Failed to read the previous workspace state: ${isError(err) ? err.message : String(err)}`)
    return undefined
  }
}

function preserveCarriedSettings (
  settings: WorkspaceStateSettings,
  previous: WorkspaceStateSettings | undefined,
  ignoredSettings: Array<keyof WorkspaceStateSettings> | undefined
): void {
  if (previous == null) return
  if (settings.autoDedupe === undefined && previous.autoDedupe !== undefined) {
    settings.autoDedupe = previous.autoDedupe
  }
  if (ignoredSettings == null) return
  const mutable = settings as Record<keyof WorkspaceStateSettings, unknown>
  for (const key of ignoredSettings) {
    if (previous[key] !== undefined) {
      mutable[key] = previous[key]
    } else {
      delete mutable[key]
    }
  }
}

/**
 * Records in `workspaceState` what `validation` proves, with the local
 * dependencies of its lockfiles. `false` when the state is not to be written:
 * an `onlyIfTrusted` record the next check would not trust.
 */
async function recordValidationToWrite (
  workspaceState: WorkspaceState,
  opts: UpdateWorkspaceStateOptions,
  validation: StateValidationOptions,
  previous: WorkspaceState | undefined
): Promise<boolean> {
  const recorded = recordStateValidation(workspaceState, opts, validation, previous)
  if (recorded != null && workspaceState.validation != null) {
    await recordLocalDependencies(workspaceState.validation, recorded, {
      previous: previous?.validation,
      read: validation.readLocalDependencies,
    })
  }
  return !validation.onlyIfTrusted || (workspaceState.validation != null && trustsEveryFile(workspaceState.validation))
}

/**
 * Records in `workspaceState` what `validation` proves. A record that cannot
 * be made leaves the state without one, which the next check proves from the
 * lockfiles.
 */
function recordStateValidation (
  workspaceState: WorkspaceState,
  opts: UpdateWorkspaceStateOptions,
  { vouchedProjects, onlyIfTrusted: _onlyIfTrusted, readLocalDependencies: _readLocalDependencies, ...validation }: StateValidationOptions,
  previous: WorkspaceState | undefined
): RecordedValidation | undefined {
  try {
    return recordValidation(workspaceState, {
      ...validation,
      allProjects: opts.allProjects,
      workspaceDir: opts.workspaceDir,
      vouchedProjects: vouchedProjects == null ? undefined : new Set(vouchedProjects),
      previous: vouchedProjects == null ? undefined : previous,
    })
  } catch (err: unknown) {
    delete workspaceState.validation
    globalWarn(`Failed to record the validation in the workspace state: ${isError(err) ? err.message : String(err)}`)
    return undefined
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
