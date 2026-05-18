import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { type LockfileObject, readWantedLockfile } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

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
 * No-op when the cache isn't wired or when no verifiers are active,
 * mirroring the gate in verifyLockfileResolutions.
 *
 * The cache compares hashes of the parsed lockfile object, not the raw
 * file bytes. The in-memory object handed to the writer can differ from
 * the one the next install parses back from disk — optional fields are
 * `undefined` in memory but absent (or `{}`) after a round-trip, which
 * `object-hash` treats as distinct values. To keep the recorded hash
 * aligned with what the next install will compute, re-read the lockfile
 * here instead of hashing the just-passed in-memory object.
 */
export async function recordLockfileVerified (opts: RecordLockfileVerifiedOptions): Promise<void> {
  if (!opts.cacheDir) return
  if (!opts.resolutionVerifiers?.length) return
  if (!opts.lockfile.packages) return
  const reloaded = await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })
  if (!reloaded) return
  recordVerification(opts.cacheDir, {
    lockfilePath: path.resolve(opts.lockfileDir, WANTED_LOCKFILE),
    verifiers: opts.resolutionVerifiers,
    hashLockfile: () => hashObject(reloaded),
  })
}
