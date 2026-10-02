import path from 'node:path'

import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { DryRunInstallResult } from '@pnpm/installing.deps-installer'
import type { Project, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { type ProjectsList, updateWorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'

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
  await updateWorkspaceState({
    allProjects: projectsToRecordInWorkspaceState(allProjects, opts, manifest),
    settings: withUpdatedCatalogs(opts, updatedCatalogs),
    workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
    pnpmfiles: opts.pnpmfile,
    filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
    configDependencies: opts.configDependencies,
  })
}

export async function recursiveInstallThenUpdateWorkspaceState (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions & WorkspaceStateSettings & Pick<InstallDepsOptions, 'saveWorkspaceState'>,
  cmdFullName: CommandFullName,
  updatedCatalogs?: Catalogs
): Promise<DryRunInstallResult | undefined> {
  const recursiveResult = await recursive(allProjects, params, opts, cmdFullName)
  if (shouldSaveWorkspaceState(opts)) {
    await updateWorkspaceState({
      allProjects,
      settings: withUpdatedCatalogs(opts, updatedCatalogs, recursiveResult.updatedCatalogs),
      workspaceDir: opts.workspaceDir,
      pnpmfiles: opts.pnpmfile,
      filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
      configDependencies: opts.configDependencies,
    })
  }
  return recursiveResult.dryRunResult
}

function shouldSaveWorkspaceState (opts: Pick<InstallDepsOptions, 'lockfileOnly' | 'saveWorkspaceState'>): boolean {
  return !opts.lockfileOnly && opts.saveWorkspaceState !== false
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
