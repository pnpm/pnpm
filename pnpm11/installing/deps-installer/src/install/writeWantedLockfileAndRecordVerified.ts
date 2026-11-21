import { type LockfileObject, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordLockfileVerified } from './recordLockfileVerified.js'
import type { ExoticSubdepsScope } from './verifyLockfileResolutions.js'

export interface WriteWantedLockfileAndRecordVerifiedOptions {
  lockfileDir: string
  lockfile: LockfileObject
  cacheDir?: string
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
  blockExoticSubdeps?: boolean
  exoticSubdepsScope?: ExoticSubdepsScope
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

/** Combines {@link writeWantedLockfile} and {@link recordLockfileVerified} — see each for semantics. */
export async function writeWantedLockfileAndRecordVerified (
  opts: WriteWantedLockfileAndRecordVerifiedOptions
): Promise<LockfileObject> {
  const written = await writeWantedLockfile(opts.lockfileDir, opts.lockfile, {
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  if (opts.cacheDir != null) {
    recordLockfileVerified({
      cacheDir: opts.cacheDir,
      lockfile: written,
      resolutionVerifiers: opts.resolutionVerifiers,
      blockExoticSubdeps: opts.blockExoticSubdeps,
      exoticSubdepsScope: opts.exoticSubdepsScope,
    })
  }
  return written
}
