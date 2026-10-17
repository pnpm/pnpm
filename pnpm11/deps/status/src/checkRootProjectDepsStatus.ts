import type fs from 'node:fs'
import path from 'node:path'

import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { type LockfileObject, readCurrentLockfile } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import type { ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { sharedInputPaths, updateWorkspaceState, type WorkspaceState } from '@pnpm/workspace.state'
import { isEmpty } from 'ramda'

import { assertLockfilesEqual } from './assertLockfilesEqual.js'
import { assertWantedLockfileUpToDate } from './assertWantedLockfileUpToDate.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { errorMessageOf, outdatedResult } from './outdatedResult.js'
import { patchesOrHooksAreModified } from './patchesOrHooksAreModified.js'
import { safeStat } from './safeStat.js'
import { statManifestFile } from './statManifestFile.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'
import { type ValidationProof, validationProof } from './validationProof.js'
import { allowUnawaitedFailure, readWantedLockfileIn, throwLockfileNotFound } from './wantedLockfiles.js'

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
  const lockfiles = await readRootProjectLockfiles(ctx)
  const { currentLockfileStats, manifestStats } = lockfiles
  const effectiveWantedLockfileStats = getEffectiveWantedLockfileStats(lockfiles, ctx)

  const { issue } = await patchesOrHooksAreModified({
    patchedDependencies: opts.patchedDependencies,
    rootDir: rootProjectManifestDir,
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

  const proof = validationProof(workspaceState, rootProjectManifestDir, ctx.wantedLockfileName)
  if (proof != null) return checkRecordedRootProject(ctx, lockfiles, proof)

  await assertLockfilesEqualWhenWantedIsNewer(lockfiles, rootProjectManifestDir)

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
    await assertNoDependenciesRequired(lockfiles.wantedLockfilePromise, rootProjectManifestDir)
  }

  return upToDateRootProjectResult(lockfiles, ctx)
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
  const projectProof = proof.proofOf(rootProjectManifestDir, rootProjectManifest)
  if (projectProof === 'proven') {
    logger.debug({ msg: 'No manifest files or lockfiles were modified since the last validation. Exiting check.' })
    if (!lockfiles.currentLockfileStats) await assertNoDependenciesRequired(lockfiles.wantedLockfilePromise, rootProjectManifestDir)
    if (proof.rehashed) await recordRootProject(ctx, true)
    return upToDateRootProjectResult(lockfiles, ctx)
  }
  logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })
  try {
    if (projectProof !== 'manifestChanged') await assertWantedLockfileInstalled(lockfiles, rootProjectManifestDir)
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
  { opts, rootProjectManifest, rootProjectManifestDir, wantedLockfileName, workspaceState }: RootProjectDepsStatusContext,
  onlyIfTrusted: boolean
): Promise<void> {
  await updateWorkspaceState({
    allProjects: [{ rootDir: rootProjectManifestDir as ProjectRootDir, manifest: rootProjectManifest }],
    workspaceDir: rootProjectManifestDir,
    pnpmfiles: workspaceState.pnpmfiles,
    settings: opts,
    filteredInstall: workspaceState.filteredInstall,
    configDependencies: workspaceState.configDependencies,
    validation: {
      lockfileDir: rootProjectManifestDir,
      wantedLockfileName,
      sharedInputs: sharedInputPaths(rootProjectManifestDir, opts.patchedDependencies, opts.pnpmfile),
      onlyIfTrusted,
    },
  })
}

/** The current lockfile records what the wanted one asks to install. */
async function assertWantedLockfileInstalled (lockfiles: RootProjectLockfiles, rootProjectManifestDir: string): Promise<void> {
  if (!lockfiles.wantedLockfileStats) return
  if (!lockfiles.currentLockfileStats) {
    await assertNoDependenciesRequired(lockfiles.wantedLockfilePromise, rootProjectManifestDir)
    return
  }
  const wantedLockfile = (await lockfiles.wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
  assertLockfilesEqual(await lockfiles.currentLockfilePromise, wantedLockfile, { wantedLockfileDir: rootProjectManifestDir })
}

async function readRootProjectLockfiles ({ opts, rootProjectManifestDir, wantedLockfileName }: RootProjectDepsStatusContext): Promise<RootProjectLockfiles> {
  const installStateDir = path.join(rootProjectManifestDir, 'node_modules', '.pnpm')
  const currentLockfilePromise = allowUnawaitedFailure(readCurrentLockfile(installStateDir, { ignoreIncompatible: false }))
  const wantedLockfilePromise = allowUnawaitedFailure(readWantedLockfileIn(rootProjectManifestDir, opts))
  const [
    currentLockfileStats,
    wantedLockfileStats,
    manifestStats,
  ] = await Promise.all([
    safeStat(path.join(installStateDir, 'lock.yaml')),
    safeStat(path.join(rootProjectManifestDir, wantedLockfileName)),
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
  { opts, rootProjectManifestDir }: RootProjectDepsStatusContext
): fs.Stats {
  if (!wantedLockfileStats && (!currentLockfileStats || opts.useGitBranchLockfile)) return throwLockfileNotFound(rootProjectManifestDir)
  return (wantedLockfileStats ?? currentLockfileStats)!
}

async function assertLockfilesEqualWhenWantedIsNewer (lockfiles: RootProjectLockfiles, rootProjectManifestDir: string): Promise<void> {
  const { currentLockfileStats, wantedLockfileStats } = lockfiles
  if (!wantedLockfileStats || !currentLockfileStats || !modifiedAtOrAfter(wantedLockfileStats, currentLockfileStats.mtime.valueOf())) return
  const currentLockfile = await lockfiles.currentLockfilePromise
  const wantedLockfile = (await lockfiles.wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
  assertLockfilesEqual(currentLockfile, wantedLockfile, { wantedLockfileDir: rootProjectManifestDir })
}

async function assertRootManifestUpToDate (
  { opts, rootProjectManifest, rootProjectManifestDir }: RootProjectDepsStatusContext,
  lockfiles: RootProjectLockfiles
): Promise<void> {
  await assertWantedLockfileUpToDate({
    autoInstallPeers: opts.autoInstallPeers,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    config: opts,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    linkWorkspacePackages: opts.linkWorkspacePackages,
    getManifestsByDir: () => ({}),
    getWorkspacePackages: () => undefined,
    rootDir: rootProjectManifestDir,
  }, {
    projectDir: rootProjectManifestDir,
    projectId: '.' as ProjectId,
    projectManifest: rootProjectManifest,
    wantedLockfile: await readEffectiveWantedLockfile(lockfiles, rootProjectManifestDir),
    wantedLockfileDir: rootProjectManifestDir,
  })
}

async function readEffectiveWantedLockfile (lockfiles: RootProjectLockfiles, rootProjectManifestDir: string): Promise<LockfileObject> {
  const lockfile = lockfiles.wantedLockfileStats == null ? await lockfiles.currentLockfilePromise : await lockfiles.wantedLockfilePromise
  return lockfile ?? throwLockfileNotFound(rootProjectManifestDir)
}

async function assertNoDependenciesRequired (wantedLockfilePromise: Promise<LockfileObject | null>, rootProjectManifestDir: string): Promise<void> {
  const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
  if (!isEmpty(wantedLockfile.packages ?? {})) {
    throw new PnpmError('RUN_CHECK_DEPS_NO_DEPS', 'The lockfile requires dependencies but none were installed', {
      hint: 'Run `pnpm install` to install dependencies',
    })
  }
}

async function upToDateRootProjectResult (
  lockfiles: RootProjectLockfiles,
  { rootProjectManifestDir, workspaceState }: RootProjectDepsStatusContext
): Promise<CheckDepsStatusResult> {
  if (lockfiles.wantedLockfileStats == null) {
    const currentLockfile = await lockfiles.currentLockfilePromise
    if (currentLockfile != null) {
      return {
        upToDate: true,
        workspaceState,
        wantedLockfileToRestore: { lockfile: currentLockfile, lockfileDir: rootProjectManifestDir },
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
