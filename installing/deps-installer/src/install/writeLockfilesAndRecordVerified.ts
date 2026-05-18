import path from 'node:path'

import { getWantedLockfileName, type LockfileObject, writeLockfiles, type WriteLockfilesResult } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordLockfileVerified } from './recordLockfileVerified.js'

export interface WriteLockfilesAndRecordVerifiedOptions {
  wantedLockfile: LockfileObject
  wantedLockfileDir: string
  currentLockfile: LockfileObject
  currentLockfileDir: string
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  cacheDir?: string
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
}

/** Plural counterpart of {@link writeWantedLockfileAndRecordVerified}. */
export async function writeLockfilesAndRecordVerified (
  opts: WriteLockfilesAndRecordVerifiedOptions
): Promise<WriteLockfilesResult> {
  const cacheActive = opts.cacheDir != null && (opts.resolutionVerifiers?.length ?? 0) > 0
  const wantedLockfileName = cacheActive
    ? await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
    : undefined
  const written = await writeLockfiles({
    wantedLockfile: opts.wantedLockfile,
    wantedLockfileDir: opts.wantedLockfileDir,
    currentLockfile: opts.currentLockfile,
    currentLockfileDir: opts.currentLockfileDir,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    wantedLockfileName,
  })
  if (cacheActive) {
    recordLockfileVerified({
      cacheDir: opts.cacheDir,
      lockfilePath: path.resolve(opts.wantedLockfileDir, wantedLockfileName!),
      lockfile: written.wantedLockfile,
      resolutionVerifiers: opts.resolutionVerifiers,
    })
  }
  return written
}
