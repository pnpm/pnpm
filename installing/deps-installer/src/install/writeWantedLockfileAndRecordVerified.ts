import path from 'node:path'

import { getWantedLockfileName, type LockfileObject, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordLockfileVerified } from './recordLockfileVerified.js'

export interface WriteWantedLockfileAndRecordVerifiedOptions {
  lockfileDir: string
  lockfile: LockfileObject
  cacheDir?: string
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  /**
   * Pre-resolved absolute path of the lockfile. Optional: when omitted
   * the wrapper calls `getWantedLockfileName` itself. Pass it when the
   * caller already computed it (e.g. for the pre-resolution gate) to
   * avoid the redundant `getCurrentBranch` shell-out.
   */
  lockfilePath?: string
}

/**
 * Convenience over {@link writeWantedLockfile} + {@link
 * recordLockfileVerified}: write the lockfile and, if a verification
 * cache is wired, record the canonical write-side hash so the next
 * install can stat- or hash-shortcut its way past the registry
 * round-trip.
 *
 * Returns the writer's canonical lockfile object — same return as the
 * raw writer, so callers that previously held onto its return value
 * can swap in this wrapper without touching anything downstream.
 *
 * The verification cache record is a no-op when the cache isn't wired
 * or no verifiers are active; in those cases this wrapper is exactly
 * equivalent to calling `writeWantedLockfile` directly (plus the
 * single `getWantedLockfileName` call when `lockfilePath` is omitted).
 */
export async function writeWantedLockfileAndRecordVerified (
  opts: WriteWantedLockfileAndRecordVerifiedOptions
): Promise<LockfileObject> {
  const lockfilePath = opts.lockfilePath ?? path.resolve(
    opts.lockfileDir,
    await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
  )
  const written = await writeWantedLockfile(opts.lockfileDir, opts.lockfile, {
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  recordLockfileVerified({
    cacheDir: opts.cacheDir,
    lockfilePath,
    lockfile: written,
    resolutionVerifiers: opts.resolutionVerifiers,
  })
  return written
}
