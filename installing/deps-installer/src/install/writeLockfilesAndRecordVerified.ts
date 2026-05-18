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
  /**
   * Pre-resolved absolute path of the wanted lockfile. Optional: when
   * omitted the wrapper calls `getWantedLockfileName` itself. Pass it
   * when the caller already computed it (e.g. for the pre-resolution
   * gate) to avoid the redundant `getCurrentBranch` shell-out.
   */
  wantedLockfilePath?: string
}

/**
 * Convenience over {@link writeLockfiles} + {@link
 * recordLockfileVerified}: write both lockfiles and, if a verification
 * cache is wired, record the canonical write-side hash of the wanted
 * lockfile so the next install can stat- or hash-shortcut its way
 * past the registry round-trip.
 *
 * Returns the writer's result unchanged. The verification cache record
 * is a no-op when the cache isn't wired or no verifiers are active,
 * so this wrapper is exactly equivalent to calling {@link writeLockfiles}
 * directly in that case (plus the single `getWantedLockfileName` call
 * when `wantedLockfilePath` is omitted).
 */
export async function writeLockfilesAndRecordVerified (
  opts: WriteLockfilesAndRecordVerifiedOptions
): Promise<WriteLockfilesResult> {
  const wantedLockfilePath = opts.wantedLockfilePath ?? path.resolve(
    opts.wantedLockfileDir,
    await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
  )
  const written = await writeLockfiles({
    wantedLockfile: opts.wantedLockfile,
    wantedLockfileDir: opts.wantedLockfileDir,
    currentLockfile: opts.currentLockfile,
    currentLockfileDir: opts.currentLockfileDir,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  recordLockfileVerified({
    cacheDir: opts.cacheDir,
    lockfilePath: wantedLockfilePath,
    lockfile: written.wantedLockfile,
    resolutionVerifiers: opts.resolutionVerifiers,
  })
  return written
}
