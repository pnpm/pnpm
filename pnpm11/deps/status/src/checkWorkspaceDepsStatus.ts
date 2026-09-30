import type fs from 'node:fs'
import path from 'node:path'

import { createProjectModulesDirResolver } from '@pnpm/config.reader'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import {
  getLockfileImporterId,
  type LockfileObject,
  readCurrentLockfile,
} from '@pnpm/lockfile.fs'
import { getWorkspacePackagesByDirectory } from '@pnpm/lockfile.verification'
import { logger } from '@pnpm/logger'
import type { Project, ProjectId } from '@pnpm/types'
import { updateWorkspaceState, type WorkspaceState } from '@pnpm/workspace.state'
import { equals, filter, once } from 'ramda'

import { assertWantedLockfileUpToDate, type AssertWantedLockfileUpToDateContext } from './assertWantedLockfileUpToDate.js'
import { findModulesDirIssue, type ProjectStats } from './findModulesDirIssue.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { errorMessageOf, outdatedResult } from './outdatedResult.js'
import { patchesOrHooksAreModified } from './patchesOrHooksAreModified.js'
import { safeStat } from './safeStat.js'
import { statManifestFile } from './statManifestFile.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'
import {
  allowUnawaitedFailure,
  assertCurrentLockfileMatches,
  missingWantedLockfileStandIn,
  readWantedLockfileIn,
  statLockfileSync,
  throwLockfileNotFound,
  type WantedLockfilesScan,
} from './wantedLockfiles.js'

export interface WorkspaceDepsStatusContext {
  opts: CheckDepsStatusOptions
  workspaceState: WorkspaceState
  allProjects: Project[]
  workspaceDir: string
  wantedLockfileName: string
  lockfilesScan: WantedLockfilesScan
}

type ReadWantedLockfileAndDir = (projectDir: string) => Promise<{
  wantedLockfile: LockfileObject
  wantedLockfileDir: string
}>

interface WantedLockfileReader {
  readWantedLockfileAndDir: ReadWantedLockfileAndDir
  wantedLockfileToRestore?: CheckDepsStatusResult['wantedLockfileToRestore']
}

export async function checkWorkspaceDepsStatus (ctx: WorkspaceDepsStatusContext): Promise<CheckDepsStatusResult> {
  const { allProjects, lockfilesScan, opts, workspaceState } = ctx
  const structureIssue = findWorkspaceStructureIssue(ctx)
  if (structureIssue != null) return outdatedResult(structureIssue, workspaceState)

  const allManifestStats = await statProjects(opts, allProjects)
  const modulesDirIssue = await findModulesDirIssue({ ...ctx, allManifestStats })
  if (modulesDirIssue != null) return outdatedResult(modulesDirIssue, workspaceState)

  const issue = await patchesOrHooksAreModified({
    patchedDependencies: opts.patchedDependencies,
    rootDir: opts.rootProjectManifestDir,
    lastValidatedTimestamp: workspaceState.lastValidatedTimestamp,
    currentPnpmfiles: opts.pnpmfile,
    previousPnpmfiles: workspaceState.pnpmfiles,
    ignorePnpmfile: opts.ignorePnpmfile,
  })
  if (issue) {
    return { upToDate: false, issue, workspaceState }
  }

  const modifiedProjects = allManifestStats.filter(
    ({ manifestStats }) =>
      modifiedAtOrAfter(manifestStats, workspaceState.lastValidatedTimestamp)
  )

  if ((modifiedProjects.length === 0) && !lockfilesScan.anyModified) {
    const unchangedResult = await checkUnchangedWorkspace(ctx)
    if (unchangedResult != null) return unchangedResult
  }

  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })

  return validateModifiedWorkspace(ctx, lockfilesScan.anyModified ? allManifestStats : modifiedProjects)
}

async function validateModifiedWorkspace (ctx: WorkspaceDepsStatusContext, projectsToCheck: ProjectStats[]): Promise<CheckDepsStatusResult> {
  const { allProjects, opts, workspaceDir, workspaceState } = ctx
  const { readWantedLockfileAndDir, wantedLockfileToRestore } = await createWantedLockfileReader(ctx)
  try {
    await assertProjectsUpToDate(ctx, projectsToCheck.map(({ project }) => project), readWantedLockfileAndDir)
  } catch (err) {
    return outdatedResult(errorMessageOf(err), workspaceState)
  }

  // update lastValidatedTimestamp to prevent pointless repeat
  await updateWorkspaceState({
    allProjects,
    workspaceDir,
    pnpmfiles: workspaceState.pnpmfiles,
    settings: opts,
    filteredInstall: workspaceState.filteredInstall,
  })

  return { upToDate: true, workspaceState, wantedLockfileToRestore }
}

function findWorkspaceStructureIssue ({ allProjects, opts, workspaceState }: WorkspaceDepsStatusContext): string | undefined {
  if (!equals(
    filter(value => value != null, workspaceState.settings.catalogs ?? {}),
    filter(value => value != null, opts.catalogs ?? {})
  )) {
    return 'Catalogs cache outdated'
  }
  if (allProjects.length !== Object.keys(workspaceState.projects).length ||
    !allProjects.every((currentProject) => {
      const prevProject = workspaceState.projects[currentProject.rootDir]
      if (!prevProject) return false
      return prevProject.name === currentProject.manifest.name && (prevProject.version ?? '0.0.0') === (currentProject.manifest.version ?? '0.0.0')
    })
  ) {
    return 'The workspace structure has changed since last install'
  }
  return undefined
}

async function statProjects (opts: CheckDepsStatusOptions, allProjects: Project[]): Promise<ProjectStats[]> {
  const statModulesDir = createModulesDirStatter(opts)
  return Promise.all(allProjects.map(async project => {
    const modulesDirStatsPromise = statModulesDir(project)
    const manifestStats = await statManifestFile(project.rootDir)
    if (!manifestStats) {
      // this error should not happen
      throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${project.rootDir}`)
    }
    return {
      project,
      manifestStats,
      modulesDirStats: await modulesDirStatsPromise,
    }
  }))
}

function createModulesDirStatter (opts: CheckDepsStatusOptions): (project: Project) => Promise<fs.Stats | undefined> {
  if (opts.nodeLinker === 'hoisted') {
    const statsPromise = safeStat(path.resolve(opts.rootProjectManifestDir, opts.modulesDir ?? 'node_modules'))
    return () => statsPromise
  }
  const _nodeLinkerTypeGuard: 'isolated' | 'pnp' | undefined = opts.nodeLinker // static type assertion
  const modulesDirOf = createProjectModulesDirResolver(opts)
  return project => safeStat(path.resolve(project.rootDir, modulesDirOf(project.manifest.name) ?? 'node_modules'))
}

/**
 * The result for a workspace in which no manifest or lockfile was modified
 * since the last validation, or `undefined` when the full check must run.
 */
async function checkUnchangedWorkspace (
  { lockfilesScan, opts, wantedLockfileName, workspaceDir, workspaceState }: WorkspaceDepsStatusContext
): Promise<CheckDepsStatusResult | undefined> {
  const wantedLockfileToRestore = lockfilesScan.anyMissing && opts.sharedWorkspaceLockfile && !opts.useGitBranchLockfile
    ? await missingWantedLockfileStandIn(workspaceDir, wantedLockfileName)
    : undefined
  // A missing wanted lockfile only skips the full check when the current
  // lockfile can stand in for it. Otherwise fall through so the checks
  // below throw RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND instead of silently
  // reporting "up to date".
  if (lockfilesScan.anyMissing && wantedLockfileToRestore == null) return undefined
  logger.debug({ msg: 'No manifest files or lockfiles were modified since the last validation. Exiting check.' })
  return { upToDate: true, workspaceState, wantedLockfileToRestore }
}

async function createWantedLockfileReader (ctx: WorkspaceDepsStatusContext): Promise<WantedLockfileReader> {
  if (!ctx.opts.sharedWorkspaceLockfile) {
    return { readWantedLockfileAndDir: createPerProjectWantedLockfileReader(ctx) }
  }
  const wantedLockfileStats = statLockfileSync(path.join(ctx.workspaceDir, ctx.wantedLockfileName))
  if (wantedLockfileStats == null) return readCurrentLockfileAsWanted(ctx)
  const { opts, workspaceDir, workspaceState } = ctx
  const wantedLockfilePromise = allowUnawaitedFailure(readWantedLockfileIn(workspaceDir, opts))
  if (modifiedAtOrAfter(wantedLockfileStats, workspaceState.lastValidatedTimestamp)) {
    await assertCurrentLockfileMatches(workspaceDir, wantedLockfilePromise, workspaceState.filteredInstall)
  }
  return {
    readWantedLockfileAndDir: async () => ({
      wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(workspaceDir),
      wantedLockfileDir: workspaceDir,
    }),
  }
}

/**
 * `pnpm-lock.yaml` is gone, but the current lockfile records exactly what the
 * previous install materialized. It stands in as the wanted lockfile for the
 * checks, and is reported back so `installDeps` can restore `pnpm-lock.yaml`
 * from it without resolving. There is no second lockfile to compare against,
 * so the wanted-vs-current equality assertion doesn't apply on this path.
 */
async function readCurrentLockfileAsWanted ({ opts, workspaceDir }: WorkspaceDepsStatusContext): Promise<WantedLockfileReader> {
  if (opts.useGitBranchLockfile) return throwLockfileNotFound(workspaceDir)
  const currentLockfile = await readCurrentLockfile(path.join(workspaceDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
  if (currentLockfile == null) return throwLockfileNotFound(workspaceDir)
  return {
    wantedLockfileToRestore: { lockfile: currentLockfile, lockfileDir: workspaceDir },
    readWantedLockfileAndDir: async () => ({
      wantedLockfile: currentLockfile,
      wantedLockfileDir: workspaceDir,
    }),
  }
}

function createPerProjectWantedLockfileReader ({ opts, wantedLockfileName, workspaceState }: WorkspaceDepsStatusContext): ReadWantedLockfileAndDir {
  return async wantedLockfileDir => {
    const wantedLockfilePromise = allowUnawaitedFailure(readWantedLockfileIn(wantedLockfileDir, opts))
    const wantedLockfileStats = await safeStat(path.join(wantedLockfileDir, wantedLockfileName))

    if (!wantedLockfileStats) return throwLockfileNotFound(wantedLockfileDir)
    if (modifiedAtOrAfter(wantedLockfileStats, workspaceState.lastValidatedTimestamp)) {
      await assertCurrentLockfileMatches(wantedLockfileDir, wantedLockfilePromise, workspaceState.filteredInstall)
    }

    return {
      wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir),
      wantedLockfileDir,
    }
  }
}

async function assertProjectsUpToDate (
  { allProjects, opts, workspaceDir }: WorkspaceDepsStatusContext,
  projectsToCheck: Project[],
  readWantedLockfileAndDir: ReadWantedLockfileAndDir
): Promise<void> {
  type GetProjectId = (project: Pick<Project, 'rootDir'>) => ProjectId
  const getProjectId: GetProjectId = opts.sharedWorkspaceLockfile
    ? project => getLockfileImporterId(workspaceDir, project.rootDir)
    : () => '.' as ProjectId

  const getWorkspacePackages = once(arrayOfWorkspacePackagesToMap.bind(null, allProjects))
  const getManifestsByDir = once(() => getWorkspacePackagesByDirectory(getWorkspacePackages()))

  const assertCtx: AssertWantedLockfileUpToDateContext = {
    autoInstallPeers: opts.autoInstallPeers,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    config: opts,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    linkWorkspacePackages: opts.linkWorkspacePackages,
    getManifestsByDir,
    getWorkspacePackages,
    rootDir: workspaceDir,
  }

  await Promise.all(projectsToCheck.map(async (project) => {
    const { wantedLockfile, wantedLockfileDir } = await readWantedLockfileAndDir(project.rootDir)
    await assertWantedLockfileUpToDate(assertCtx, {
      projectDir: project.rootDir,
      projectId: getProjectId(project),
      projectManifest: project.manifest,
      wantedLockfile,
      wantedLockfileDir,
    })
  }))
}
