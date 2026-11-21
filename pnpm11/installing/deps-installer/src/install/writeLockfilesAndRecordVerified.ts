import { type LockfileObject, writeLockfiles, type WriteLockfilesResult } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordLockfileVerified } from './recordLockfileVerified.js'
import type { ExoticSubdepsScope } from './verifyLockfileResolutions.js'

export interface WriteLockfilesAndRecordVerifiedOptions {
  wantedLockfile: LockfileObject
  wantedLockfileDir: string
  currentLockfile: LockfileObject
  currentLockfileDir: string
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  cacheDir?: string
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
  blockExoticSubdeps?: boolean
  exoticSubdepsScope?: ExoticSubdepsScope
}

/** Plural counterpart of {@link writeWantedLockfileAndRecordVerified}. */
export async function writeLockfilesAndRecordVerified (
  opts: WriteLockfilesAndRecordVerifiedOptions
): Promise<WriteLockfilesResult> {
  const written = await writeLockfiles({
    wantedLockfile: opts.wantedLockfile,
    wantedLockfileDir: opts.wantedLockfileDir,
    currentLockfile: opts.currentLockfile,
    currentLockfileDir: opts.currentLockfileDir,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  if (opts.cacheDir != null) {
    recordLockfileVerified({
      cacheDir: opts.cacheDir,
      lockfile: written.wantedLockfile,
      resolutionVerifiers: opts.resolutionVerifiers,
      blockExoticSubdeps: opts.blockExoticSubdeps,
      exoticSubdepsScope: opts.exoticSubdepsScope,
    })
  }
  return written
}
