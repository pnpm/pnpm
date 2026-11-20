import fs from 'node:fs'
import path from 'node:path'

import { createProjectModulesDirResolver } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import {
  getGitBranchLockfileNamesSync,
  getLockfileImporterId,
  type LockfileObject,
  readCurrentLockfile,
  readWantedLockfile,
  wantedLockfileHasMergeConflictsSync,
} from '@pnpm/lockfile.fs'
import type { IncludedDependencies, Project, ProjectId } from '@pnpm/types'

import { assertLockfilesEqual, type AssertLockfilesEqualOptions, importerClosuresMatch } from './assertLockfilesEqual.js'
import { declaresDependencies, isProjectRecordedInCurrentLockfile, notInstalledIssue } from './findModulesDirIssue.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { safeStatSync } from './safeStat.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'

export type WantedLockfileReadOptions = Pick<CheckDepsStatusOptions, 'modulesDir' | 'packageConfigs' | 'useGitBranchLockfile' | 'mergeGitBranchLockfiles'>

export interface UnprovenProject {
  project: Project
  unrecorded: boolean
}

export interface LockfileChangedCheckInput {
  lockfileDir: string
  projects: UnprovenProject[]
  include: IncludedDependencies
  sharedWorkspaceLockfile?: boolean
  readOpts: WantedLockfileReadOptions
}

export interface LockfileChangedCheckResult {
  issue?: string
  wantedLockfile?: LockfileObject | null
  wantedLockfilesByDir?: Map<string, LockfileObject | null>
}

export function currentLockfileDir (lockfileDir: string, modulesDir?: string): string {
  return path.join(path.resolve(lockfileDir, modulesDir ?? 'node_modules'), '.pnpm')
}

export async function findOutdatedLockfileChangedProject (input: LockfileChangedCheckInput): Promise<LockfileChangedCheckResult> {
  if (input.projects.length === 0) return {}
  if (input.sharedWorkspaceLockfile === false) {
    return findOutdatedPerProjectLockfile(input)
  }
  const { include, lockfileDir, projects, readOpts } = input
  const [wanted, current] = await Promise.all([
    readWantedLockfileIn(lockfileDir, readOpts),
    readCurrentLockfile(currentLockfileDir(lockfileDir, readOpts.modulesDir), { ignoreIncompatible: false }),
  ])
  if (wanted == null) return { wantedLockfile: null }
  const allImporterIds = projects.map(({ project }) => getLockfileImporterId(lockfileDir, project.rootDir))
  if (importerClosuresMatch(wanted, current, allImporterIds, include)) {
    return { wantedLockfile: wanted }
  }
  for (const { project, unrecorded } of projects) {
    const importerId = getLockfileImporterId(lockfileDir, project.rootDir)
    if (!importerClosuresMatch(wanted, current, [importerId], include)) {
      return { issue: unprovenProjectIssue(project, unrecorded), wantedLockfile: wanted }
    }
  }
  return { wantedLockfile: wanted }
}

async function findOutdatedPerProjectLockfile ({ include, projects, readOpts }: LockfileChangedCheckInput): Promise<LockfileChangedCheckResult> {
  const modulesDirOf = createProjectModulesDirResolver(readOpts)
  const wantedLockfilesByDir = new Map<string, LockfileObject | null>()
  const checks = await Promise.all(projects.map(async ({ project, unrecorded }) => {
    const [wanted, current] = await Promise.all([
      readWantedLockfileIn(project.rootDir, readOpts),
      readCurrentLockfile(currentLockfileDir(project.rootDir, modulesDirOf(project.manifest.name)), { ignoreIncompatible: false }),
    ])
    wantedLockfilesByDir.set(project.rootDir, wanted)
    const outdated = wanted != null && !importerClosuresMatch(wanted, current, ['.' as ProjectId], include)
    return outdated ? unprovenProjectIssue(project, unrecorded) : undefined
  }))
  return { issue: checks.find((issue) => issue != null), wantedLockfilesByDir }
}

function unprovenProjectIssue (project: Project, unrecorded: boolean): string {
  const name = project.manifest.name ?? project.rootDir
  return unrecorded
    ? `Workspace package ${name} has dependencies but was not installed`
    : `The installed dependencies of ${name} are not up to date with the lockfile`
}

export function readWantedLockfileIn (lockfileDir: string, opts: WantedLockfileReadOptions): Promise<LockfileObject | null> {
  return readWantedLockfile(lockfileDir, {
    ignoreIncompatible: false,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
}

/**
 * Keeps a lockfile read that starts before it is known to be needed from
 * crashing the process with an unhandled rejection when it is never awaited.
 * Awaiting the returned promise still throws the read's error.
 */
export function allowUnawaitedFailure<Result> (promise: Promise<Result>): Promise<Result> {
  promise.catch(() => undefined)
  return promise
}

/**
 * Reads the current lockfile of `wantedLockfileDir` and throws when it does
 * not match the wanted lockfile.
 */
export async function assertCurrentLockfileMatches (
  wantedLockfileDir: string,
  wantedLockfilePromise: Promise<LockfileObject | null>,
  opts: Omit<AssertLockfilesEqualOptions, 'wantedLockfileDir'> & { modulesDir?: string }
): Promise<void> {
  const currentLockfile = await readCurrentLockfile(currentLockfileDir(wantedLockfileDir, opts.modulesDir), { ignoreIncompatible: false })
  const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir)
  assertLockfilesEqual(currentLockfile, wantedLockfile, {
    wantedLockfileDir,
    ...opts,
  })
}

export function throwLockfileNotFound (wantedLockfileDir: string): never {
  throw new PnpmError('RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND', `Cannot find a lockfile in ${wantedLockfileDir}`, {
    hint: 'Run `pnpm install` to create the lockfile',
  })
}

export function assertCurrentLockfileRecordsProjects (
  currentLockfile: LockfileObject,
  lockfileDir: string,
  projects: Project[]
): void {
  const notInstalled = projects.find((project) =>
    declaresDependencies(project) &&
    !isProjectRecordedInCurrentLockfile(currentLockfile, getLockfileImporterId(lockfileDir, project.rootDir)))
  if (notInstalled != null) {
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_DEPS', notInstalledIssue(notInstalled))
  }
}

/**
 * When `<lockfileDir>/pnpm-lock.yaml` is missing but the current lockfile
 * exists, returns the current lockfile so the caller can restore
 * `pnpm-lock.yaml` from it. `undefined` when the wanted lockfile is present
 * (nothing to restore) or when there is no current lockfile to restore from.
 */
export async function missingWantedLockfileStandIn (
  lockfileDir: string,
  wantedLockfileName: string,
  modulesDir?: string
): Promise<CheckDepsStatusResult['wantedLockfileToRestore']> {
  if (safeStatSync(path.join(lockfileDir, wantedLockfileName)) != null) return undefined
  const currentLockfile = await readCurrentLockfile(currentLockfileDir(lockfileDir, modulesDir), { ignoreIncompatible: false })
  if (currentLockfile == null) return undefined
  return { lockfile: currentLockfile, lockfileDir }
}

export function getWantedLockfileDirs (
  opts: Pick<CheckDepsStatusOptions, 'allProjects' | 'lockfileDir' | 'rootProjectManifestDir' | 'sharedWorkspaceLockfile' | 'workspaceDir'>
): string[] {
  if (opts.allProjects && opts.workspaceDir && opts.sharedWorkspaceLockfile === false) {
    return [...new Set(opts.allProjects.map(({ rootDir }) => rootDir))]
  }
  return [opts.lockfileDir ?? opts.workspaceDir ?? opts.rootProjectManifestDir]
}

export interface WantedLockfilesScan {
  conflictedDir: string | undefined
  anyModified: boolean
  /** The lockfile directories without a wanted lockfile. */
  missingDirs: string[]
}

interface WantedLockfilesScanOptions {
  wantedLockfileName: string
  mergeGitBranchLockfiles?: boolean
}

export function scanWantedLockfiles (lockfileDirs: string[], lastValidatedTimestamp: number, opts: WantedLockfilesScanOptions): WantedLockfilesScan {
  let anyModified = false
  const missingDirs: string[] = []
  for (const lockfileDir of lockfileDirs) {
    const dirScan = scanWantedLockfilesInDir(lockfileDir, lastValidatedTimestamp, opts)
    anyModified ||= dirScan.anyModified
    if (dirScan.conflicted) {
      return { conflictedDir: lockfileDir, anyModified, missingDirs }
    }
    if (!dirScan.found) missingDirs.push(lockfileDir)
  }
  return { conflictedDir: undefined, anyModified, missingDirs }
}

interface WantedLockfilesInDirScan {
  found: boolean
  anyModified: boolean
  conflicted: boolean
}

function scanWantedLockfilesInDir (lockfileDir: string, lastValidatedTimestamp: number, opts: WantedLockfilesScanOptions): WantedLockfilesInDirScan {
  // With `mergeGitBranchLockfiles`, `readWantedLockfile` merges every
  // `pnpm-lock.*.yaml`, so a change in any of them changes the wanted
  // lockfile and must be detected here.
  const lockfileNames = opts.mergeGitBranchLockfiles
    ? gitBranchLockfileNames(lockfileDir, opts.wantedLockfileName)
    : [opts.wantedLockfileName]
  const scan: WantedLockfilesInDirScan = { found: false, anyModified: false, conflicted: false }
  for (const lockfileName of lockfileNames) {
    const stats = statLockfileSync(path.join(lockfileDir, lockfileName))
    if (stats == null) continue
    scan.found = true
    if (!modifiedAtOrAfter(stats, lastValidatedTimestamp)) continue
    scan.anyModified = true
    if (stats.isFile() && wantedLockfileHasMergeConflictsSync(lockfileDir, lockfileName)) {
      scan.conflicted = true
      return scan
    }
  }
  return scan
}

/** The lockfile's stats, or `undefined` when it does not exist. */
export function statLockfileSync (lockfilePath: string): fs.Stats | undefined {
  try {
    return fs.statSync(lockfilePath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  }
}

/**
 * Whether a branch lockfile waits to be merged into the wanted lockfile. An
 * install with `mergeGitBranchLockfiles` merges and deletes them, so one left
 * means the merged lockfile may differ from the one that install recorded.
 */
export function hasUnmergedBranchLockfiles (lockfileDir: string, wantedLockfileName: string): boolean {
  return gitBranchLockfileNames(lockfileDir, wantedLockfileName).some((name) => name !== wantedLockfileName)
}

function gitBranchLockfileNames (lockfileDir: string, wantedLockfileName: string): string[] {
  let branchLockfileNames: string[]
  try {
    branchLockfileNames = getGitBranchLockfileNamesSync(lockfileDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      branchLockfileNames = []
    } else {
      throw err
    }
  }
  return branchLockfileNames.includes(wantedLockfileName)
    ? branchLockfileNames
    : [wantedLockfileName, ...branchLockfileNames]
}
