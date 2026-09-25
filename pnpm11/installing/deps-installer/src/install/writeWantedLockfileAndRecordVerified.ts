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
}

/** Combines {@link writeWantedLockfile} and {@link recordLockfileVerified} — see each for semantics. */
export async function writeWantedLockfileAndRecordVerified (
  opts: WriteWantedLockfileAndRecordVerifiedOptions
): Promise<LockfileObject> {
  const cacheActive = opts.cacheDir != null && (opts.resolutionVerifiers?.length ?? 0) > 0
  const lockfileName = cacheActive
    ? await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
    : undefined
  const written = await writeWantedLockfile(opts.lockfileDir, opts.lockfile, {
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    lockfileName,
  })
  if (cacheActive) {
    recordLockfileVerified({
      cacheDir: opts.cacheDir,
      lockfilePath: path.resolve(opts.lockfileDir, lockfileName!),
      lockfile: written,
      resolutionVerifiers: opts.resolutionVerifiers,
    })
  }
  return written
}
