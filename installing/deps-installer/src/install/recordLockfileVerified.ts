import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { hashLockfile } from './lockfileHash.js'
import { recordVerification } from './verifyLockfileResolutionsCache.js'

export interface RecordLockfileVerifiedOptions {
  cacheDir?: string
  lockfileDir: string
  lockfile: LockfileObject
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
}

/**
 * Records a post-resolution lockfile in the verification cache so the
 * next install with an unchanged lockfile takes the stat/hash fast path
 * instead of re-checking every entry against the registry.
 *
 * Safe to call because fresh local resolution already enforces the
 * policy: the resolver's per-version filter
 * (resolving/npm-resolver/src/pickPackage.ts) rejects picks the verifier
 * would reject, and any entries carried over from the pre-resolution
 * lockfile already passed the gate at the top of mutateModules. So
 * every entry in the just-written lockfile is policy-clean by
 * construction; we record that fact instead of re-discovering it.
 *
 * Hashes the lockfile via {@link hashLockfile} so the recorded hash
 * matches what the next install will compute on its loaded copy
 * without re-reading the file we just wrote.
 *
 * No-op when the cache isn't wired or when no verifiers are active,
 * mirroring the gate in verifyLockfileResolutions.
 */
export function recordLockfileVerified (opts: RecordLockfileVerifiedOptions): void {
  if (!opts.cacheDir) return
  if (!opts.resolutionVerifiers?.length) return
  if (!opts.lockfile.packages) return
  recordVerification(opts.cacheDir, {
    lockfilePath: path.resolve(opts.lockfileDir, WANTED_LOCKFILE),
    verifiers: opts.resolutionVerifiers,
    hashLockfile: () => hashLockfile(opts.lockfile),
  })
}
