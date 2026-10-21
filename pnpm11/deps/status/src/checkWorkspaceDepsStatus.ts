import type fs from 'node:fs'
import path from 'node:path'

import { createProjectModulesDirResolver } from '@pnpm/config.reader'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { getLockfileImporterId, type LockfileObject, readCurrentLockfile } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import type { Project, ProjectId } from '@pnpm/types'
import type { WorkspaceState } from '@pnpm/workspace.state'
import { equals, filter } from 'ramda'

import { assertWorkspaceProjectsUpToDate } from './assertWantedLockfileUpToDate.js'
import { findModulesDirIssue, type ProjectStats, selectProjectDirs } from './findModulesDirIssue.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { errorMessageOf, outdatedResult } from './outdatedResult.js'
import { patchesOrHooksAreModified } from './patchesOrHooksAreModified.js'
import { safeStat } from './safeStat.js'
import { statManifestFile } from './statManifestFile.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'
import { recordValidatedWorkspaceState, type ValidationProof, validationProof } from './validationProof.js'
import {
  allowUnawaitedFailure, assertCurrentLockfileMatches, assertCurrentLockfileRecordsProjects,
  currentLockfileDir, findOutdatedLockfileChangedProject, getWantedLockfileDirs,
  hasUnmergedBranchLockfiles, missingWantedLockfileStandIn, readWantedLockfileIn,
  statLockfileSync, throwLockfileNotFound, type WantedLockfilesScan,
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
  const proof = opts.sharedWorkspaceLockfile === false && coveredProjectsMissingLockfile(ctx, allManifestStats).length > 0
    ? undefined
    : validationProof(workspaceState, workspaceLockfileDir(ctx), {
      wantedLockfileName: ctx.wantedLockfileName,
      sharedWorkspaceLockfile: opts.sharedWorkspaceLockfile,
      unmergedBranchLockfiles: opts.sharedWorkspaceLockfile !== false && opts.mergeGitBranchLockfiles === true && hasUnmergedBranchLockfiles(workspaceLockfileDir(ctx), ctx.wantedLockfileName),
      hasUnmergedBranchLockfiles: (dir) => opts.mergeGitBranchLockfiles === true && hasUnmergedBranchLockfiles(dir, ctx.wantedLockfileName),
    })
  const modulesDirIssue = await findModulesDirIssue({ ...ctx, allManifestStats, hasValidationProof: proof != null })
  if (modulesDirIssue != null) return outdatedResult(modulesDirIssue, workspaceState)

  const { issue, patchesRevalidated } = await patchesOrHooksStatus(ctx)
  if (issue) return { upToDate: false, issue, workspaceState }

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
  const installCovers = createInstallCoverage(ctx)
  const coveredManifestStats = allManifestStats.filter(({ project }) => installCovers(project.rootDir))
  const modifiedProjects = coveredManifestStats.filter(
    ({ manifestStats }) =>
      modifiedAtOrAfter(manifestStats, workspaceState.lastValidatedTimestamp)
  )

  if (!workspaceState.filteredInstall && (modifiedProjects.length === 0) && !lockfilesScan.anyModified && !patchesRevalidated) {
    const unchangedResult = await checkUnchangedWorkspace(ctx, allManifestStats)
    if (unchangedResult != null) {
      await recordUnchangedLegacyStateIfNeeded(ctx, coveredManifestStats)
      return unchangedResult
    }
  }

  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })

  const projectsToCheck = workspaceState.filteredInstall || lockfilesScan.anyModified
    ? coveredManifestStats
    : [...new Set([...modifiedProjects, ...coveredProjectsMissingLockfile(ctx, coveredManifestStats)])]
  return validateModifiedWorkspace(ctx, projectsToCheck, {
    vouchedProjects: coveredManifestStats.map(({ project }) => project.rootDir),
  })
}

async function recordUnchangedLegacyStateIfNeeded (ctx: WorkspaceDepsStatusContext, coveredManifestStats: ProjectStats[]): Promise<void> {
  if (ctx.workspaceState.validation == null && !ctx.workspaceState.filteredInstall) {
    await recordVouchedWorkspaceState(ctx, coveredManifestStats.map(({ project }) => project.rootDir))
  }
}

/**
 * With a lockfile per project, the projects without one among those the
 * install the gate spawns covers. Checking such a project reports its missing
 * lockfile. A filtered install leaves the projects it did not select without
 * a lockfile, and the install the gate spawns keeps that filter, so those are
 * not held.
 */
function coveredProjectsMissingLockfile (ctx: WorkspaceDepsStatusContext, allManifestStats: ProjectStats[]): ProjectStats[] {
  const { lockfilesScan, opts } = ctx
  if (opts.sharedWorkspaceLockfile !== false) return []
  const missingDirs = new Set(lockfilesScan.missingDirs.map((dir) => path.resolve(dir)))
  const installCovers = createInstallCoverage(ctx)
  return allManifestStats.filter(({ project }) => missingDirs.has(path.resolve(project.rootDir)) && installCovers(project.rootDir))
}

/** What the content check of a workspace covers. */
interface VouchedCheck {
  /** The projects a passing check proves installed. */
  vouchedProjects: string[]
  /** Whether a recorded validation already checked whether the lockfile changed. */
  lockfileAlreadyChecked?: boolean
  wantedLockfile?: LockfileObject | null
  wantedLockfilesByDir?: Map<string, LockfileObject | null>
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
  vouched: VouchedCheck
): Promise<CheckDepsStatusResult> {
  const { workspaceState } = ctx
  // A recorded check has already held every project whose lockfile changed.
  const { readWantedLockfileAndDir, wantedLockfileToRestore } = await createWantedLockfileReader({
    ctx,
    projectsToCheck: projectsToCheck.map(({ project }) => project),
    lockfileModified: vouched.lockfileAlreadyChecked ? false : (workspaceState.filteredInstall ? true : undefined),
    preloadedWantedLockfile: vouched.wantedLockfile,
    preloadedWantedLockfilesByDir: vouched.wantedLockfilesByDir,
  })
  try {
    await assertWorkspaceProjectsUpToDate(
      { allProjects: ctx.allProjects, lockfileDir: workspaceLockfileDir(ctx), opts: ctx.opts },
      projectsToCheck.map(({ project }) => project),
      readWantedLockfileAndDir
    )
  } catch (err) {
    return outdatedResult(errorMessageOf(err), workspaceState)
  }

  // update lastValidatedTimestamp to prevent pointless repeat
  await recordVouchedWorkspaceState(ctx, vouched.vouchedProjects)

  return { upToDate: true, workspaceState, wantedLockfileToRestore }
}

async function recordVouchedWorkspaceState (
  ctx: WorkspaceDepsStatusContext,
  vouchedProjects: string[],
  onlyIfTrusted?: boolean
): Promise<void> {
  const { allProjects, opts, wantedLockfileName, workspaceState } = ctx
  await recordValidatedWorkspaceState({
    allProjects,
    lockfileDir: workspaceLockfileDir(ctx),
    opts,
    workspaceState,
    wantedLockfileName,
    vouchedProjects,
    onlyIfTrusted,
  })
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
  const lockfileCheck = proof.lockfileUnreadable
    ? { issue: 'Cannot check whether dependencies are outdated' }
    : await checkLockfileChangedProjects(ctx, held, proof)
  if (lockfileCheck.issue) return outdatedResult(lockfileCheck.issue, workspaceState)
  const unchanged = held.length === 0 && !patchesRevalidated
    ? await checkUnchangedWorkspace(ctx, allManifestStats)
    : undefined
  if (unchanged != null) {
    if (proof.rehashed) await recordVouchedWorkspaceState(ctx, provenProjects(allManifestStats, proof), true)
    return unchanged
  }
  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })
  return validateModifiedWorkspace(ctx, held, {
    vouchedProjects: [...held.map(({ project }) => project.rootDir), ...provenProjects(allManifestStats, proof)],
    lockfileAlreadyChecked: true,
    wantedLockfile: lockfileCheck.wantedLockfile,
    wantedLockfilesByDir: lockfileCheck.wantedLockfilesByDir,
  })
}

async function checkLockfileChangedProjects (
  ctx: WorkspaceDepsStatusContext,
  held: ProjectStats[],
  proof: ValidationProof
): ReturnType<typeof findOutdatedLockfileChangedProject> {
  return findOutdatedLockfileChangedProject({
    lockfileDir: workspaceLockfileDir(ctx),
    projects: held.flatMap(({ project }) => {
      const projectProof = proof.proofOf(project.rootDir, project.manifest)
      return projectProof === 'manifestChanged' ? [] : [{ project, unrecorded: projectProof === 'unrecorded' }]
    }),
    include: workspaceInclude(ctx),
    sharedWorkspaceLockfile: ctx.opts.sharedWorkspaceLockfile,
    readOpts: ctx.opts,
  })
}

function provenProjects (allManifestStats: ProjectStats[], proof: ValidationProof): string[] {
  return allManifestStats
    .filter(({ project }) => proof.proofOf(project.rootDir, project.manifest) === 'proven')
    .map(({ project }) => project.rootDir)
}

/** The projects the gate holds that are not proven installed. */
function heldProjects (ctx: WorkspaceDepsStatusContext, allManifestStats: ProjectStats[], proof: ValidationProof): ProjectStats[] {
  const { lockfilesScan, opts } = ctx
  const installCovers = createInstallCoverage(ctx)
  const missingDirs = opts.sharedWorkspaceLockfile === false
    ? new Set(lockfilesScan.missingDirs.map((dir) => path.resolve(dir)))
    : undefined
  return allManifestStats.filter(({ project }) => {
    const covered = installCovers(project.rootDir)
    if (opts.selectedProjectsGraph != null && !covered) return false
    if (missingDirs?.has(path.resolve(project.rootDir)) && !covered) return false
    const projectProof = proof.proofOf(project.rootDir, project.manifest)
    if (projectProof === 'proven') return false
    return projectProof !== 'unrecorded' || covered
  })
}

/**
 * Whether the install the gate spawns covers the project at `rootDir`: a
 * filtered command's install selects the same projects, a non-recursive
 * command's install covers the command's project and its workspace
 * dependencies, and without either the install covers the whole workspace.
 */
function createInstallCoverage (
  ctx: WorkspaceDepsStatusContext
): (rootDir: string) => boolean {
  const { allProjects, opts, workspaceState } = ctx
  if (opts.selectedProjectsGraph == null && opts.dir == null) return () => true
  const selected = selectProjectDirs(allProjects, {
    ...opts,
    autoInstallPeers: opts.autoInstallPeers !== false && workspaceState.settings.autoInstallPeers !== false,
    include: workspaceInclude(ctx),
  })
  return (rootDir) => selected.has(path.resolve(rootDir))
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
    if (!manifestStats) throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${project.rootDir}`)
    return { project, manifestStats, modulesDirStats: await modulesDirStatsPromise }
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
  ctx: WorkspaceDepsStatusContext,
  allManifestStats: ProjectStats[]
): Promise<CheckDepsStatusResult | undefined> {
  const { lockfilesScan, opts, wantedLockfileName, workspaceState } = ctx
  const anyMissing = opts.sharedWorkspaceLockfile === false
    ? coveredProjectsMissingLockfile(ctx, allManifestStats).length > 0
    : lockfilesScan.missingDirs.length > 0
  const wantedLockfileToRestore = anyMissing && opts.sharedWorkspaceLockfile !== false && !opts.useGitBranchLockfile
    ? await missingWantedLockfileStandIn(workspaceLockfileDir(ctx), wantedLockfileName, opts.modulesDir)
    : undefined
  // A missing wanted lockfile only skips the full check when the current
  // lockfile can stand in for it. Otherwise fall through so the checks
  // below throw RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND instead of silently
  // reporting "up to date".
  if (anyMissing && wantedLockfileToRestore == null) return undefined
  logger.debug({ msg: 'No manifest files or lockfiles were modified since the last validation. Exiting check.' })
  return { upToDate: true, workspaceState, wantedLockfileToRestore }
}

async function readWorkspaceLockfiles (ctx: WorkspaceDepsStatusContext): Promise<Array<LockfileObject | null>> {
  const { opts } = ctx
  if (opts.sharedWorkspaceLockfile === false) {
    return Promise.all(getWantedLockfileDirs(opts).map((lockfileDir) => readWantedLockfileIn(lockfileDir, opts)))
  }
  const lockfileDir = workspaceLockfileDir(ctx)
  return [
    (await readWantedLockfileIn(lockfileDir, opts)) ??
      await readCurrentLockfile(currentLockfileDir(lockfileDir, opts.modulesDir), { ignoreIncompatible: false }),
  ]
}

function workspaceInclude ({ workspaceState: { settings } }: WorkspaceDepsStatusContext) {
  return { dependencies: settings.production !== false, devDependencies: settings.dev !== false, optionalDependencies: settings.optional !== false }
}

interface CreateWantedLockfileReaderOptions {
  ctx: WorkspaceDepsStatusContext
  projectsToCheck: Project[]
  /**
   * Overrides the modification time when the recorded validation already
   * decided whether the lockfile changed.
   */
  lockfileModified?: boolean
  preloadedWantedLockfile?: LockfileObject | null
  preloadedWantedLockfilesByDir?: Map<string, LockfileObject | null>
}

async function createWantedLockfileReader (
  { ctx, projectsToCheck, lockfileModified, preloadedWantedLockfile, preloadedWantedLockfilesByDir }: CreateWantedLockfileReaderOptions
): Promise<WantedLockfileReader> {
  if (ctx.opts.sharedWorkspaceLockfile === false) {
    return { readWantedLockfileAndDir: createPerProjectWantedLockfileReader(ctx, lockfileModified, preloadedWantedLockfilesByDir) }
  }
  const lockfileDir = workspaceLockfileDir(ctx)
  const wantedLockfileStats = statLockfileSync(path.join(lockfileDir, ctx.wantedLockfileName))
  if (wantedLockfileStats == null) return readCurrentLockfileAsWanted(ctx, projectsToCheck)
  const { opts, workspaceState } = ctx
  const wantedLockfilePromise = preloadedWantedLockfile !== undefined
    ? Promise.resolve(preloadedWantedLockfile)
    : allowUnawaitedFailure(readWantedLockfileIn(lockfileDir, opts))
  if (lockfileModified ?? modifiedAtOrAfter(wantedLockfileStats, workspaceState.lastValidatedTimestamp)) {
    const importersToCheck = projectsToCheck.map((project) => getLockfileImporterId(lockfileDir, project.rootDir))
    await assertCurrentLockfileMatches(lockfileDir, wantedLockfilePromise, { filteredInstall: workspaceState.filteredInstall, importersToCheck, include: workspaceInclude(ctx), modulesDir: opts.modulesDir })
  }
  return {
    readWantedLockfileAndDir: async () => ({
      wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(lockfileDir),
      wantedLockfileDir: lockfileDir,
    }),
  }
}

/**
 * `pnpm-lock.yaml` is gone, but the current lockfile records what the previous
 * install materialized. It stands in as the wanted lockfile for the checks,
 * and is reported back so `installDeps` can restore `pnpm-lock.yaml` from it
 * without resolving.
 */
async function readCurrentLockfileAsWanted (
  ctx: WorkspaceDepsStatusContext,
  projectsToCheck: Project[]
): Promise<WantedLockfileReader> {
  const { opts, workspaceState } = ctx
  const lockfileDir = workspaceLockfileDir(ctx)
  if (opts.useGitBranchLockfile) return throwLockfileNotFound(lockfileDir)
  const currentLockfile = await readCurrentLockfile(currentLockfileDir(lockfileDir, opts.modulesDir), { ignoreIncompatible: false })
  if (currentLockfile == null) return throwLockfileNotFound(lockfileDir)
  if (workspaceState.filteredInstall) {
    assertCurrentLockfileRecordsProjects(currentLockfile, lockfileDir, projectsToCheck)
  }
  return {
    wantedLockfileToRestore: { lockfile: currentLockfile, lockfileDir },
    readWantedLockfileAndDir: async () => ({ wantedLockfile: currentLockfile, wantedLockfileDir: lockfileDir }),
  }
}

function resolveProjectModulesDir (ctx: WorkspaceDepsStatusContext, projectDir: string): string | undefined {
  const modulesDirOf = createProjectModulesDirResolver(ctx.opts)
  const project = ctx.allProjects.find(({ rootDir }) => path.resolve(rootDir) === path.resolve(projectDir))
  return modulesDirOf(project?.manifest.name)
}

function createPerProjectWantedLockfileReader (
  ctx: WorkspaceDepsStatusContext,
  lockfileModified?: boolean,
  preloadedByDir?: Map<string, LockfileObject | null>
): ReadWantedLockfileAndDir {
  const { opts, wantedLockfileName, workspaceState } = ctx
  const include = workspaceInclude(ctx)
  return async wantedLockfileDir => {
    const preloaded = preloadedByDir?.get(wantedLockfileDir)
    const wantedLockfilePromise = preloaded !== undefined
      ? Promise.resolve(preloaded)
      : allowUnawaitedFailure(readWantedLockfileIn(wantedLockfileDir, opts))
    const wantedLockfileStats = await safeStat(path.join(wantedLockfileDir, wantedLockfileName))
    if (!wantedLockfileStats) return throwLockfileNotFound(wantedLockfileDir)
    if (lockfileModified ?? modifiedAtOrAfter(wantedLockfileStats, workspaceState.lastValidatedTimestamp)) {
      await assertCurrentLockfileMatches(wantedLockfileDir, wantedLockfilePromise, { filteredInstall: workspaceState.filteredInstall, importersToCheck: ['.' as ProjectId], include, modulesDir: resolveProjectModulesDir(ctx, wantedLockfileDir) })
    }
    return { wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir), wantedLockfileDir }
  }
}
