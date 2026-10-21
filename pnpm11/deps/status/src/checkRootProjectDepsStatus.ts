import type fs from 'node:fs'
import path from 'node:path'

import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, type LockfileObject, readCurrentLockfile } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import type { WorkspaceState } from '@pnpm/workspace.state'
import { isEmpty } from 'ramda'

import { assertLockfilesEqual } from './assertLockfilesEqual.js'
import { assertWantedLockfileUpToDate } from './assertWantedLockfileUpToDate.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { errorMessageOf, outdatedResult } from './outdatedResult.js'
import { patchesOrHooksAreModified } from './patchesOrHooksAreModified.js'
import { safeStat } from './safeStat.js'
import { statManifestFile } from './statManifestFile.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'
import { recordValidatedWorkspaceState, type ValidationProof, validationProof } from './validationProof.js'
import { allowUnawaitedFailure, hasUnmergedBranchLockfiles, readWantedLockfileIn, throwLockfileNotFound } from './wantedLockfiles.js'

export interface RootProjectDepsStatusContext {
  opts: CheckDepsStatusOptions
  workspaceState: WorkspaceState
  wantedLockfileName: string
  rootProjectManifest: ProjectManifest
  rootProjectManifestDir: string
}

interface RootProjectLockfiles {
  currentLockfilePromise: Promise<LockfileObject | null>
  wantedLockfilePromise: Promise<LockfileObject | null>
  currentLockfileStats: fs.Stats | undefined
  wantedLockfileStats: fs.Stats | undefined
  manifestStats: fs.Stats | undefined
}

export async function checkRootProjectDepsStatus (ctx: RootProjectDepsStatusContext): Promise<CheckDepsStatusResult> {
  const { opts, rootProjectManifestDir, workspaceState } = ctx
  if (recordedInAnotherDirectory(workspaceState, rootProjectManifestDir)) {
    return outdatedResult('The project directory has changed since last install', workspaceState)
  }
  const lockfileDir = rootLockfileDir(ctx)
  const lockfiles = await readRootProjectLockfiles(ctx)
  const { currentLockfileStats, manifestStats } = lockfiles
  const effectiveWantedLockfileStats = getEffectiveWantedLockfileStats(lockfiles, ctx)

  const { issue } = await patchesOrHooksAreModified({
    patchedDependencies: opts.patchedDependencies,
    rootDir: lockfileDir,
    lastValidatedTimestamp: effectiveWantedLockfileStats.mtime.valueOf(),
    validation: workspaceState.validation,
    currentPnpmfiles: opts.pnpmfile,
    previousPnpmfiles: workspaceState.pnpmfiles,
    ignorePnpmfile: opts.ignorePnpmfile,
    readLockfiles: async () => [(await lockfiles.wantedLockfilePromise) ?? await lockfiles.currentLockfilePromise],
  })
  if (issue) {
    return { upToDate: false, issue, workspaceState }
  }

  const proof = recordedProof(ctx)
  if (proof != null) return checkRecordedRootProject(ctx, lockfiles, proof)

  await assertLockfilesEqualWhenWantedIsNewer(lockfiles, lockfileDir)

  if (!manifestStats) {
    // this error should not happen
    throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${rootProjectManifestDir}`)
  }

  if (modifiedAtOrAfter(manifestStats, effectiveWantedLockfileStats.mtime.valueOf())) {
    logger.debug({ msg: 'The manifest is newer than the lockfile. Continuing check.' })
    try {
      await assertRootManifestUpToDate(ctx, lockfiles)
    } catch (err) {
      return outdatedResult(errorMessageOf(err), workspaceState)
    }
  } else if (currentLockfileStats) {
    logger.debug({ msg: 'The manifest file is not newer than the lockfile. Exiting check.' })
  } else {
    await assertNoDependenciesRequired(lockfiles.wantedLockfilePromise, lockfileDir)
  }

  return upToDateRootProjectResult(lockfiles, ctx)
}

function rootLockfileDir ({ opts, rootProjectManifestDir }: RootProjectDepsStatusContext): string {
  return opts.lockfileDir ?? rootProjectManifestDir
}

function recordedProof (ctx: RootProjectDepsStatusContext): ValidationProof | undefined {
  const { opts, wantedLockfileName, workspaceState } = ctx
  const lockfileDir = rootLockfileDir(ctx)
  return validationProof(workspaceState, lockfileDir, {
    wantedLockfileName,
    unmergedBranchLockfiles: opts.mergeGitBranchLockfiles === true && hasUnmergedBranchLockfiles(lockfileDir, wantedLockfileName),
  })
}

/**
 * The check of a project whose state records what the last validation proved
 * it installed with. A passing check records the project again, so the next
 * run need not repeat it.
 */
async function checkRecordedRootProject (
  ctx: RootProjectDepsStatusContext,
  lockfiles: RootProjectLockfiles,
  proof: ValidationProof
): Promise<CheckDepsStatusResult> {
  const { rootProjectManifest, rootProjectManifestDir, workspaceState } = ctx
  if (proof.lockfileUnreadable) return outdatedResult('Cannot check whether dependencies are outdated', workspaceState)
  const lockfileDir = rootLockfileDir(ctx)
  const projectProof = proof.proofOf(rootProjectManifestDir, rootProjectManifest)
  if (projectProof === 'proven') {
    logger.debug({ msg: 'No manifest files or lockfiles were modified since the last validation. Exiting check.' })
    if (!lockfiles.currentLockfileStats) await assertNoDependenciesRequired(lockfiles.wantedLockfilePromise, lockfileDir)
    if (proof.rehashed) await recordRootProject(ctx, true)
    return upToDateRootProjectResult(lockfiles, ctx)
  }
  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })
  try {
    await assertWantedLockfileInstalled(lockfiles, lockfileDir)
    await assertRootManifestUpToDate(ctx, lockfiles)
  } catch (err) {
    return outdatedResult(errorMessageOf(err), workspaceState)
  }
  await recordRootProject(ctx, false)
  return upToDateRootProjectResult(lockfiles, ctx)
}

/**
 * Records the project as proven installed. `onlyIfTrusted` is set for a
 * refresh that only records the lockfile's stat again.
 */
async function recordRootProject (
  ctx: RootProjectDepsStatusContext,
  onlyIfTrusted: boolean
): Promise<void> {
  const { opts, rootProjectManifest, rootProjectManifestDir, wantedLockfileName, workspaceState } = ctx
  await recordValidatedWorkspaceState({
    allProjects: [{ rootDir: rootProjectManifestDir as ProjectRootDir, manifest: rootProjectManifest }],
    lockfileDir: rootLockfileDir(ctx),
    opts,
    workspaceState,
    wantedLockfileName,
    onlyIfTrusted,
  })
}

/** The current lockfile records what the wanted one asks to install. */
async function assertWantedLockfileInstalled (lockfiles: RootProjectLockfiles, lockfileDir: string): Promise<void> {
  if (!lockfiles.wantedLockfileStats) return
  if (!lockfiles.currentLockfileStats) {
    await assertNoDependenciesRequired(lockfiles.wantedLockfilePromise, lockfileDir)
    return
  }
  const wantedLockfile = (await lockfiles.wantedLockfilePromise) ?? throwLockfileNotFound(lockfileDir)
  assertLockfilesEqual(await lockfiles.currentLockfilePromise, wantedLockfile, { wantedLockfileDir: lockfileDir })
}

async function readRootProjectLockfiles (ctx: RootProjectDepsStatusContext): Promise<RootProjectLockfiles> {
  const { opts, rootProjectManifestDir, wantedLockfileName } = ctx
  const lockfileDir = rootLockfileDir(ctx)
  const installStateDir = path.join(lockfileDir, opts.modulesDir ?? 'node_modules', '.pnpm')
  const currentLockfilePromise = allowUnawaitedFailure(readCurrentLockfile(installStateDir, { ignoreIncompatible: false }))
  const wantedLockfilePromise = allowUnawaitedFailure(readWantedLockfileIn(lockfileDir, opts))
  const [
    currentLockfileStats,
    wantedLockfileStats,
    manifestStats,
  ] = await Promise.all([
    safeStat(path.join(installStateDir, 'lock.yaml')),
    safeStat(path.join(lockfileDir, wantedLockfileName)),
    statManifestFile(rootProjectManifestDir),
  ])
  return { currentLockfilePromise, wantedLockfilePromise, currentLockfileStats, wantedLockfileStats, manifestStats }
}

/**
 * When `pnpm-lock.yaml` is gone but the current lockfile
 * (`node_modules/.pnpm/lock.yaml`) survives, the current one stands
 * in as the wanted lockfile: it records exactly what the previous
 * install materialized, so the checks run against it and the
 * caller can restore `pnpm-lock.yaml` from it without resolving.
 * The wanted-vs-current equality assertion doesn't apply on this
 * path — the two are the same object.
 */
function getEffectiveWantedLockfileStats (
  { currentLockfileStats, wantedLockfileStats }: RootProjectLockfiles,
  ctx: RootProjectDepsStatusContext
): fs.Stats {
  if (!wantedLockfileStats && (!currentLockfileStats || ctx.opts.useGitBranchLockfile)) return throwLockfileNotFound(rootLockfileDir(ctx))
  return (wantedLockfileStats ?? currentLockfileStats)!
}

async function assertLockfilesEqualWhenWantedIsNewer (lockfiles: RootProjectLockfiles, lockfileDir: string): Promise<void> {
  const { currentLockfileStats, wantedLockfileStats } = lockfiles
  if (!wantedLockfileStats || !currentLockfileStats || !modifiedAtOrAfter(wantedLockfileStats, currentLockfileStats.mtime.valueOf())) return
  const currentLockfile = await lockfiles.currentLockfilePromise
  const wantedLockfile = (await lockfiles.wantedLockfilePromise) ?? throwLockfileNotFound(lockfileDir)
  assertLockfilesEqual(currentLockfile, wantedLockfile, { wantedLockfileDir: lockfileDir })
}

async function assertRootManifestUpToDate (
  ctx: RootProjectDepsStatusContext,
  lockfiles: RootProjectLockfiles
): Promise<void> {
  const { opts, rootProjectManifest, rootProjectManifestDir } = ctx
  const lockfileDir = rootLockfileDir(ctx)
  await assertWantedLockfileUpToDate({
    autoInstallPeers: opts.autoInstallPeers,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    config: opts,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    linkWorkspacePackages: opts.linkWorkspacePackages,
    getManifestsByDir: () => ({}),
    getWorkspacePackages: () => undefined,
    rootDir: lockfileDir,
  }, {
    projectDir: rootProjectManifestDir,
    projectId: getLockfileImporterId(lockfileDir, rootProjectManifestDir),
    projectManifest: rootProjectManifest,
    wantedLockfile: await readEffectiveWantedLockfile(lockfiles, lockfileDir),
    wantedLockfileDir: lockfileDir,
  })
}

async function readEffectiveWantedLockfile (lockfiles: RootProjectLockfiles, lockfileDir: string): Promise<LockfileObject> {
  const lockfile = lockfiles.wantedLockfileStats == null ? await lockfiles.currentLockfilePromise : await lockfiles.wantedLockfilePromise
  return lockfile ?? throwLockfileNotFound(lockfileDir)
}

async function assertNoDependenciesRequired (wantedLockfilePromise: Promise<LockfileObject | null>, lockfileDir: string): Promise<void> {
  const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(lockfileDir)
  if (!isEmpty(wantedLockfile.packages ?? {})) {
    throw new PnpmError('RUN_CHECK_DEPS_NO_DEPS', 'The lockfile requires dependencies but none were installed', {
      hint: 'Run `pnpm install` to install dependencies',
    })
  }
}

async function upToDateRootProjectResult (
  lockfiles: RootProjectLockfiles,
  ctx: RootProjectDepsStatusContext
): Promise<CheckDepsStatusResult> {
  const { workspaceState } = ctx
  if (lockfiles.wantedLockfileStats == null) {
    const currentLockfile = await lockfiles.currentLockfilePromise
    if (currentLockfile != null) {
      return {
        upToDate: true,
        workspaceState,
        wantedLockfileToRestore: { lockfile: currentLockfile, lockfileDir: rootLockfileDir(ctx) },
      }
    }
  }
  return { upToDate: true, workspaceState }
}

/**
 * `projectsToRecordInWorkspaceState` in `@pnpm/installing.commands` explains
 * why a moved project needs a real install. A state file without a recorded
 * project is never reported as recorded elsewhere.
 */
function recordedInAnotherDirectory (workspaceState: WorkspaceState, projectDir: string): boolean {
  const recordedProjectDirs = Object.keys(workspaceState.projects)
  return recordedProjectDirs.length > 0 &&
    !recordedProjectDirs.some(dir => path.relative(dir, projectDir) === '')
}
