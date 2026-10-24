import path from 'node:path'

import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import type { DryRunInstallResult } from '@pnpm/installing.deps-installer'
import { getWantedLockfileName, type LockfileObject, readWantedLockfile } from '@pnpm/lockfile.fs'
import { findOutdatedRegistryKey } from '@pnpm/lockfile.utils'
import type { Project, ProjectManifest, ProjectRootDir, RegistryContext } from '@pnpm/types'
import { type ProjectsList, recordedRegistryPrefixes, sharedInputPaths, type StateValidationOptions, updateWorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'

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
    settings: {
      ...withUpdatedCatalogs(opts, updatedCatalogs),
      registryPrefixes: await verifiedRegistryPrefixes(opts, async () => [await readLockfileForState(opts.lockfileDir ?? opts.workspaceDir ?? opts.dir, opts)]),
    },
    workspaceDir: opts.lockfileDir ?? opts.workspaceDir ?? opts.dir,
    storeBaseDir: opts.workspaceDir ?? opts.dir,
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
  // The install replaces `storeDir` in `opts` with the resolved store, and
  // the state must record the configured value that the next check reads.
  const configuredStoreDir = opts.storeDir
  const recursiveResult = await recursive(allProjects, params, opts, cmdFullName)
  if (recursiveResult.installedProjectDirs.length > 0 && shouldSaveWorkspaceState(opts)) {
    const filteredInstall = isFilteredInstall(allProjects, recursiveResult.installedProjectDirs)
    await updateWorkspaceState({
      allProjects,
      settings: {
        ...withUpdatedCatalogs(opts, updatedCatalogs, recursiveResult.updatedCatalogs),
        storeDir: configuredStoreDir,
        registryPrefixes: await verifiedRegistryPrefixes(opts, async () => readWorkspaceLockfiles(allProjects, opts)),
      },
      workspaceDir: opts.lockfileDir ?? opts.workspaceDir,
      storeBaseDir: opts.workspaceDir ?? opts.dir,
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

type LockfileReadOptions = Pick<InstallDepsOptions, 'mergeGitBranchLockfiles' | 'useGitBranchLockfile'>

/**
 * The `registryPrefixes` this install records: the declared prefixes, when
 * the keys of every lockfile it left behind carry them. The lockfiles are
 * read back from disk, so the keys of importers a filtered install did not
 * resolve are verified too. A missing lockfile verifies nothing, unless the
 * install keeps none: then every key is minted from the current registries.
 */
async function verifiedRegistryPrefixes (
  opts: Partial<RegistryContext> & Pick<InstallDepsOptions, 'useLockfile'>,
  readLockfiles: () => Promise<Array<LockfileObject | null>>
): Promise<Record<string, string> | undefined> {
  const declared = recordedRegistryPrefixes(opts.registriesByPrefix)
  if (declared == null || opts.useLockfile === false) return declared
  const { registriesByScope } = opts
  if (registriesByScope == null) return undefined
  const registries = pickRegistryContext({ ...opts, registriesByScope })
  const lockfiles = await readLockfiles()
  const keysAreCurrent = lockfiles.every((lockfile) => lockfile != null && findOutdatedRegistryKey(lockfile, registries) == null)
  return keysAreCurrent ? declared : undefined
}

/**
 * The lockfiles a recursive install leaves behind: the shared one, or else
 * the one of each project that has one.
 */
async function readWorkspaceLockfiles (
  allProjects: Project[],
  opts: LockfileReadOptions & Pick<RecursiveOptions, 'lockfileDir' | 'sharedWorkspaceLockfile' | 'workspaceDir'>
): Promise<Array<LockfileObject | null>> {
  if (opts.sharedWorkspaceLockfile !== false) return [await readLockfileForState(opts.lockfileDir ?? opts.workspaceDir, opts)]
  const lockfiles = await Promise.all(allProjects.map(async ({ rootDir }) => readLockfileForState(rootDir, opts)))
  // A project without a lockfile of its own has no keys to verify.
  return lockfiles.filter((lockfile) => lockfile != null)
}

async function readLockfileForState (lockfileDir: string, opts: LockfileReadOptions): Promise<LockfileObject | null> {
  return readWantedLockfile(lockfileDir, {
    ignoreIncompatible: true,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
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
