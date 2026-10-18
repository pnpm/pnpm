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
import { sharedInputPaths, updateWorkspaceState, type WorkspaceState } from '@pnpm/workspace.state'
import { equals, filter, once } from 'ramda'

import { assertWantedLockfileUpToDate, type AssertWantedLockfileUpToDateContext } from './assertWantedLockfileUpToDate.js'
import { declaresDependencies, findModulesDirIssue, type ProjectStats } from './findModulesDirIssue.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { errorMessageOf, outdatedResult } from './outdatedResult.js'
import { patchesOrHooksAreModified } from './patchesOrHooksAreModified.js'
import { safeStat } from './safeStat.js'
import { statManifestFile } from './statManifestFile.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'
import { type ValidationProof, validationProof } from './validationProof.js'
import {
  allowUnawaitedFailure,
  assertCurrentLockfileMatches,
  getWantedLockfileDirs,
  hasUnmergedBranchLockfiles,
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
  const { allProjects, opts, workspaceState } = ctx
  const structureIssue = findWorkspaceStructureIssue(ctx)
  if (structureIssue != null) return outdatedResult(structureIssue, workspaceState)

  const allManifestStats = await statProjects(opts, allProjects)
  const modulesDirIssue = await findModulesDirIssue({ ...ctx, allManifestStats })
  if (modulesDirIssue != null) return outdatedResult(modulesDirIssue, workspaceState)

  const { issue, patchesRevalidated } = await patchesOrHooksStatus(ctx)
  if (issue) {
    return { upToDate: false, issue, workspaceState }
  }

  const proof = opts.sharedWorkspaceLockfile === false
    ? undefined
    : validationProof(workspaceState, workspaceLockfileDir(ctx), {
      wantedLockfileName: ctx.wantedLockfileName,
      unmergedBranchLockfiles: opts.mergeGitBranchLockfiles === true && hasUnmergedBranchLockfiles(workspaceLockfileDir(ctx), ctx.wantedLockfileName),
    })
  return proof == null
    ? checkModifiedSinceLastValidation(ctx, allManifestStats, patchesRevalidated)
    : checkRecordedWorkspace(ctx, { proof, allManifestStats, patchesRevalidated })
}

/**
 * The check of a workspace whose state records no validation: a manifest or a
 * lockfile newer than the last validation is checked against the lockfile.
 */
async function checkModifiedSinceLastValidation (
  ctx: WorkspaceDepsStatusContext,
  allManifestStats: ProjectStats[],
  patchesRevalidated: boolean
): Promise<CheckDepsStatusResult> {
  const { lockfilesScan, workspaceState } = ctx
  const modifiedProjects = allManifestStats.filter(
    ({ manifestStats }) =>
      modifiedAtOrAfter(manifestStats, workspaceState.lastValidatedTimestamp)
  )

  if ((modifiedProjects.length === 0) && !lockfilesScan.anyModified && !patchesRevalidated) {
    const unchangedResult = await checkUnchangedWorkspace(ctx)
    if (unchangedResult != null) return unchangedResult
  }

  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })

  const projectsToCheck = lockfilesScan.anyModified
    ? allManifestStats
    : [...new Set([...modifiedProjects, ...coveredProjectsMissingLockfile(ctx, allManifestStats)])]
  return validateModifiedWorkspace(ctx, projectsToCheck)
}

/**
 * With a lockfile per project, the projects without one among those the
 * install the gate spawns covers. Checking such a project reports its missing
 * lockfile. A filtered install leaves the projects it did not select without
 * a lockfile, and the install the gate spawns keeps that filter, so those are
 * not held.
 */
function coveredProjectsMissingLockfile ({ lockfilesScan, opts }: WorkspaceDepsStatusContext, allManifestStats: ProjectStats[]): ProjectStats[] {
  if (opts.sharedWorkspaceLockfile !== false) return []
  const missingDirs = new Set(lockfilesScan.missingDirs.map((dir) => path.resolve(dir)))
  const installCovers = createInstallCoverage(opts)
  return allManifestStats.filter(({ project }) => missingDirs.has(path.resolve(project.rootDir)) && installCovers(project.rootDir))
}

/** What the content check of a recorded workspace covers. */
interface RecordedCheck {
  /** The projects a passing check proves installed. */
  vouchedProjects: string[]
}

function patchesOrHooksStatus (ctx: WorkspaceDepsStatusContext): ReturnType<typeof patchesOrHooksAreModified> {
  const { opts, workspaceState } = ctx
  return patchesOrHooksAreModified({
    patchedDependencies: opts.patchedDependencies,
    rootDir: opts.rootProjectManifestDir,
    lastValidatedTimestamp: workspaceState.lastValidatedTimestamp,
    validation: workspaceState.validation,
    currentPnpmfiles: opts.pnpmfile,
    previousPnpmfiles: workspaceState.pnpmfiles,
    ignorePnpmfile: opts.ignorePnpmfile,
    readLockfiles: () => readWorkspaceLockfiles(ctx),
  })
}

async function validateModifiedWorkspace (
  ctx: WorkspaceDepsStatusContext,
  projectsToCheck: ProjectStats[],
  recorded?: RecordedCheck
): Promise<CheckDepsStatusResult> {
  const { allProjects, opts, workspaceDir, workspaceState } = ctx
  // A recorded check has already held every project whose lockfile changed.
  const { readWantedLockfileAndDir, wantedLockfileToRestore } = await createWantedLockfileReader(ctx, recorded != null ? false : undefined)
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
    configDependencies: workspaceState.configDependencies,
    validation: recorded == null
      ? undefined
      : {
        lockfileDir: workspaceLockfileDir(ctx),
        wantedLockfileName: ctx.wantedLockfileName,
        sharedInputs: sharedInputPaths(workspaceLockfileDir(ctx), opts.patchedDependencies, opts.pnpmfile),
        vouchedProjects: recorded.vouchedProjects,
      },
  })

  return { upToDate: true, workspaceState, wantedLockfileToRestore }
}

function workspaceLockfileDir ({ opts, workspaceDir }: WorkspaceDepsStatusContext): string {
  return opts.lockfileDir ?? workspaceDir
}

interface RecordedWorkspace {
  proof: ValidationProof
  allManifestStats: ProjectStats[]
  patchesRevalidated: boolean
}

/**
 * The check of a workspace whose state records which projects the last
 * validation proved installed. A gate holds only the projects the install it
 * leads to can fix: a filtered command's install selects the same projects
 * and the workspace projects they depend on. Without a selection the install
 * covers the whole workspace, so a project whose recorded install went stale
 * is held anywhere, but one never installed only where the command runs.
 */
async function checkRecordedWorkspace (
  ctx: WorkspaceDepsStatusContext,
  { proof, allManifestStats, patchesRevalidated }: RecordedWorkspace
): Promise<CheckDepsStatusResult> {
  const { workspaceState } = ctx
  const held = heldProjects(ctx, allManifestStats, proof)
  const unprovable = proof.lockfileUnreadable
    ? 'Cannot check whether dependencies are outdated'
    : findUnprovableProject(held, proof)
  if (unprovable) return outdatedResult(unprovable, workspaceState)
  const unchanged = held.length === 0 && !patchesRevalidated
    ? await checkUnchangedWorkspace(ctx)
    : undefined
  if (unchanged != null) {
    if (proof.rehashed) await recordTrustedRefresh(ctx, provenProjects(allManifestStats, proof))
    return unchanged
  }
  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })
  return validateModifiedWorkspace(ctx, held, {
    vouchedProjects: [...held.map(({ project }) => project.rootDir), ...provenProjects(allManifestStats, proof)],
  })
}

function provenProjects (allManifestStats: ProjectStats[], proof: ValidationProof): string[] {
  return allManifestStats
    .filter(({ project }) => proof.proofOf(project.rootDir, project.manifest) === 'proven')
    .map(({ project }) => project.rootDir)
}

/**
 * Records the lockfile's stat again after its content was read and found
 * unchanged, so the next check need not read it, once the record would be
 * trusted.
 */
async function recordTrustedRefresh (ctx: WorkspaceDepsStatusContext, vouchedProjects: string[]): Promise<void> {
  const { allProjects, opts, workspaceDir, workspaceState } = ctx
  await updateWorkspaceState({
    allProjects,
    workspaceDir,
    pnpmfiles: workspaceState.pnpmfiles,
    settings: opts,
    filteredInstall: workspaceState.filteredInstall,
    configDependencies: workspaceState.configDependencies,
    validation: {
      lockfileDir: workspaceLockfileDir(ctx),
      wantedLockfileName: ctx.wantedLockfileName,
      sharedInputs: sharedInputPaths(workspaceLockfileDir(ctx), opts.patchedDependencies, opts.pnpmfile),
      vouchedProjects,
      onlyIfTrusted: true,
    },
  })
}

/** The projects the gate holds that are not proven installed. */
function heldProjects ({ opts }: WorkspaceDepsStatusContext, allManifestStats: ProjectStats[], proof: ValidationProof): ProjectStats[] {
  const installCovers = createInstallCoverage(opts)
  return allManifestStats.filter(({ project }) => {
    const projectProof = proof.proofOf(project.rootDir, project.manifest)
    if (projectProof === 'proven') return false
    return (opts.selectedProjectsGraph == null && projectProof !== 'unrecorded') || installCovers(project.rootDir)
  })
}

/**
 * Whether the install the gate spawns covers the project at `rootDir`: a
 * filtered command's install selects the same projects, a non-recursive
 * command's install runs in the command's directory, and without either the
 * install covers the whole workspace.
 */
function createInstallCoverage (opts: Pick<CheckDepsStatusOptions, 'dir' | 'selectedProjectsGraph'>): (rootDir: string) => boolean {
  const selected = opts.selectedProjectsGraph == null
    ? undefined
    : new Set(Object.keys(opts.selectedProjectsGraph).map((dir) => path.resolve(dir)))
  const commandDir = opts.dir == null ? undefined : path.resolve(opts.dir)
  return (rootDir) => {
    const resolvedRootDir = path.resolve(rootDir)
    if (selected != null) return selected.has(resolvedRootDir)
    return commandDir == null || commandDir === resolvedRootDir
  }
}

/**
 * The first held project its recorded validation cannot vouch for: one never
 * installed, or one installed from another lockfile. The current lockfile
 * cannot prove either installed, since a filtered install records the
 * projects it left out in it as if it had installed them.
 */
function findUnprovableProject (held: ProjectStats[], proof: ValidationProof): string | undefined {
  for (const { project } of held) {
    switch (proof.proofOf(project.rootDir, project.manifest)) {
      case 'unrecorded':
        if (declaresDependencies(project)) {
          return `Workspace package ${project.manifest.name ?? project.rootDir} has dependencies but was not installed`
        }
        break
      case 'lockfileChanged':
        return `The installed dependencies of ${project.manifest.name ?? project.rootDir} are not up to date with the lockfile`
      default:
        break
    }
  }
  return undefined
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
  const anyMissing = lockfilesScan.missingDirs.length > 0
  const wantedLockfileToRestore = anyMissing && opts.sharedWorkspaceLockfile && !opts.useGitBranchLockfile
    ? await missingWantedLockfileStandIn(workspaceDir, wantedLockfileName)
    : undefined
  // A missing wanted lockfile only skips the full check when the current
  // lockfile can stand in for it. Otherwise fall through so the checks
  // below throw RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND instead of silently
  // reporting "up to date".
  if (anyMissing && wantedLockfileToRestore == null) return undefined
  logger.debug({ msg: 'No manifest files or lockfiles were modified since the last validation. Exiting check.' })
  return { upToDate: true, workspaceState, wantedLockfileToRestore }
}

async function readWorkspaceLockfiles ({ opts, workspaceDir }: WorkspaceDepsStatusContext): Promise<Array<LockfileObject | null>> {
  if (!opts.sharedWorkspaceLockfile) {
    return Promise.all(getWantedLockfileDirs(opts).map((lockfileDir) => readWantedLockfileIn(lockfileDir, opts)))
  }
  return [
    (await readWantedLockfileIn(workspaceDir, opts)) ??
      await readCurrentLockfile(path.join(workspaceDir, 'node_modules/.pnpm'), { ignoreIncompatible: false }),
  ]
}

/**
 * `lockfileModified` overrides the modification time when the recorded
 * validation already decided whether the lockfile changed.
 */
async function createWantedLockfileReader (ctx: WorkspaceDepsStatusContext, lockfileModified?: boolean): Promise<WantedLockfileReader> {
  if (!ctx.opts.sharedWorkspaceLockfile) {
    return { readWantedLockfileAndDir: createPerProjectWantedLockfileReader(ctx) }
  }
  const wantedLockfileStats = statLockfileSync(path.join(ctx.workspaceDir, ctx.wantedLockfileName))
  if (wantedLockfileStats == null) return readCurrentLockfileAsWanted(ctx)
  const { opts, workspaceDir, workspaceState } = ctx
  const wantedLockfilePromise = allowUnawaitedFailure(readWantedLockfileIn(workspaceDir, opts))
  if (lockfileModified ?? modifiedAtOrAfter(wantedLockfileStats, workspaceState.lastValidatedTimestamp)) {
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
