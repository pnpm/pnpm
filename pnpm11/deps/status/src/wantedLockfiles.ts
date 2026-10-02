import fs from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'
import {
  getGitBranchLockfileNamesSync,
  type LockfileObject,
  readCurrentLockfile,
  readWantedLockfile,
  wantedLockfileHasMergeConflictsSync,
} from '@pnpm/lockfile.fs'

import { assertLockfilesEqual } from './assertLockfilesEqual.js'
import { modifiedAtOrAfter } from './modifiedAtOrAfter.js'
import { safeStatSync } from './safeStat.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'

export type WantedLockfileReadOptions = Pick<CheckDepsStatusOptions, 'useGitBranchLockfile' | 'mergeGitBranchLockfiles'>

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
  filteredInstall: boolean | undefined
): Promise<void> {
  const currentLockfile = await readCurrentLockfile(path.join(wantedLockfileDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
  const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir)
  assertLockfilesEqual(currentLockfile, wantedLockfile, {
    wantedLockfileDir,
    filteredInstall,
  })
}

export function throwLockfileNotFound (wantedLockfileDir: string): never {
  throw new PnpmError('RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND', `Cannot find a lockfile in ${wantedLockfileDir}`, {
    hint: 'Run `pnpm install` to create the lockfile',
  })
}

/**
 * When `<lockfileDir>/pnpm-lock.yaml` is missing but the current lockfile
 * exists, returns the current lockfile so the caller can restore
 * `pnpm-lock.yaml` from it. `undefined` when the wanted lockfile is present
 * (nothing to restore) or when there is no current lockfile to restore from.
 */
export async function missingWantedLockfileStandIn (lockfileDir: string, wantedLockfileName: string): Promise<CheckDepsStatusResult['wantedLockfileToRestore']> {
  if (safeStatSync(path.join(lockfileDir, wantedLockfileName)) != null) return undefined
  const currentLockfile = await readCurrentLockfile(path.join(lockfileDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
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
  anyMissing: boolean
}

interface WantedLockfilesScanOptions {
  wantedLockfileName: string
  mergeGitBranchLockfiles?: boolean
}

export function scanWantedLockfiles (lockfileDirs: string[], lastValidatedTimestamp: number, opts: WantedLockfilesScanOptions): WantedLockfilesScan {
  let anyModified = false
  let anyMissing = false
  for (const lockfileDir of lockfileDirs) {
    const dirScan = scanWantedLockfilesInDir(lockfileDir, lastValidatedTimestamp, opts)
    anyModified ||= dirScan.anyModified
    if (dirScan.conflicted) {
      return { conflictedDir: lockfileDir, anyModified, anyMissing }
    }
    if (!dirScan.found) anyMissing = true
  }
  return { conflictedDir: undefined, anyModified, anyMissing }
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
    if (wantedLockfileHasMergeConflictsSync(lockfileDir, lockfileName)) {
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
