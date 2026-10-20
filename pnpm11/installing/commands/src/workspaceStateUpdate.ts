import path from 'node:path'

import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { DryRunInstallResult } from '@pnpm/installing.deps-installer'
import { getWantedLockfileName } from '@pnpm/lockfile.fs'
import type { Project, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { type ProjectsList, sharedInputPaths, type StateValidationOptions, updateWorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'

import type { InstallDepsOptions } from './installDeps.js'
import { type CommandFullName, recursive, type RecursiveOptions } from './recursive.js'

/**
 * A single-project install has no workspace projects, so it records the
 * project itself. `checkDepsStatus` compares that path with the current one to
 * notice a project that was moved or renamed together with its
 * `node_modules`, whose links may point at the old location.
 */
function projectsToRecordInWorkspaceState (
  allProjects: Project[],
  opts: Pick<InstallDepsOptions, 'dir' | 'lockfileDir' | 'workspaceDir'>,
  manifest: ProjectManifest
): ProjectsList {
  if (allProjects.length > 0 || opts.workspaceDir != null) return allProjects
  const rootDir = opts.lockfileDir ?? opts.dir
  if (path.relative(rootDir, opts.dir) !== '') return allProjects
  return [{ rootDir: rootDir as ProjectRootDir, manifest }]
}

export interface SingleProjectWorkspaceStateUpdate {
  opts: InstallDepsOptions
  allProjects: Project[]
  manifest: ProjectManifest
  updatedCatalogs?: Catalogs
}

export async function updateSingleProjectWorkspaceState (
  { opts, allProjects, manifest, updatedCatalogs }: SingleProjectWorkspaceStateUpdate
): Promise<void> {
  if (!shouldSaveWorkspaceState(opts)) return
  const installedProjectDirs = [opts.dir]
  const filteredInstall = isFilteredInstall(allProjects, installedProjectDirs)
  await updateWorkspaceState({
    allProjects: projectsToRecordInWorkspaceState(allProjects, opts, manifest),
    settings: withUpdatedCatalogs(opts, updatedCatalogs),
    workspaceDir: opts.lockfileDir ?? opts.workspaceDir ?? opts.dir,
    pnpmfiles: opts.pnpmfile,
    filteredInstall,
    configDependencies: opts.configDependencies,
    validation: await stateValidation({ ...opts, installedProjectDirs }, filteredInstall),
  })
}

export async function recursiveInstallThenUpdateWorkspaceState (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions & WorkspaceStateSettings & Pick<InstallDepsOptions, 'lockfileCheck' | 'saveWorkspaceState'>,
  cmdFullName: CommandFullName,
  updatedCatalogs?: Catalogs
): Promise<DryRunInstallResult | undefined> {
  const recursiveResult = await recursive(allProjects, params, opts, cmdFullName)
  if (recursiveResult.installedProjectDirs.length > 0 && shouldSaveWorkspaceState(opts)) {
    const filteredInstall = isFilteredInstall(allProjects, recursiveResult.installedProjectDirs)
    await updateWorkspaceState({
      allProjects,
      settings: withUpdatedCatalogs(opts, updatedCatalogs, recursiveResult.updatedCatalogs),
      workspaceDir: opts.lockfileDir ?? opts.workspaceDir,
      pnpmfiles: opts.pnpmfile,
      filteredInstall,
      configDependencies: opts.configDependencies,
      validation: await stateValidation({
        ...opts,
        dir: opts.workspaceDir,
        installedProjectDirs: recursiveResult.installedProjectDirs,
      }, filteredInstall),
    })
  }
  return recursiveResult.dryRunResult
}

function isFilteredInstall (allProjects: Project[], installedProjectDirs: string[]): boolean {
  const installedDirs = new Set(installedProjectDirs)
  return allProjects.some((project) => !installedDirs.has(project.rootDir))
}

/** The projects and shared inputs this install proved up to date. */
interface StateValidationInputs {
  dir: string
  installedProjectDirs: string[]
  lockfileDir?: string
  workspaceDir?: string
  sharedWorkspaceLockfile?: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  patchedDependencies?: Record<string, string>
  pnpmfile: string[]
}

async function stateValidation (
  opts: StateValidationInputs,
  filteredInstall: boolean
): Promise<StateValidationOptions | undefined> {
  const lockfileDir = opts.lockfileDir ?? opts.workspaceDir ?? opts.dir
  return {
    lockfileDir,
    wantedLockfileName: await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
      cwd: lockfileDir,
    }),
    sharedInputs: sharedInputPaths(lockfileDir, opts.patchedDependencies, opts.pnpmfile),
    vouchedProjects: filteredInstall ? opts.installedProjectDirs : undefined,
    sharedWorkspaceLockfile: opts.sharedWorkspaceLockfile,
  }
}

/**
 * A check-only install (`pnpm dedupe --check`, `--dry-run`) links nothing, so
 * it proves nothing installed.
 */
function shouldSaveWorkspaceState (
  opts: Pick<InstallDepsOptions, 'dryRun' | 'lockfileCheck' | 'lockfileOnly' | 'saveWorkspaceState'>
): boolean {
  return !opts.lockfileOnly && opts.lockfileCheck == null && opts.dryRun !== true && opts.saveWorkspaceState !== false
}

/**
 * Folds the catalog entries written to `pnpm-workspace.yaml` during this
 * install into the catalogs read at startup. The workspace state cache records
 * these so a later install detects when a catalog entry was reverted; without
 * this, the cache would keep the stale pre-install catalogs and report
 * "Already up to date" even though the manifest changed.
 */
function withUpdatedCatalogs<Settings extends { catalogs?: Catalogs }> (
  settings: Settings,
  ...updatedCatalogs: Array<Catalogs | undefined>
): Settings {
  if (updatedCatalogs.every((catalogs) => catalogs == null)) return settings
  return { ...settings, catalogs: mergeCatalogs(settings.catalogs, ...updatedCatalogs) }
}
