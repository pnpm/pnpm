import { hashObject } from '@pnpm/crypto.object-hasher'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordVerification } from './verifyLockfileResolutionsCache.js'

export interface RecordLockfileVerifiedOptions {
  cacheDir?: string
  /**
   * Absolute path of the lockfile that was just written. Must match
   * the path the next install will read from — under
   * `useGitBranchLockfile` that is the branch-suffixed filename, not
   * the default `pnpm-lock.yaml`. Resolve via `getWantedLockfileName`
   * before calling.
   */
  lockfilePath: string
  /**
   * The post-write canonical lockfile object — i.e. the value returned
   * by `writeWantedLockfile` / `writeLockfiles`, not the in-memory
   * object handed to those functions. The writer YAML-round-trips its
   * output so this value is structurally identical to what the next
   * install's `readWantedLockfile` will produce, which is what makes
   * `hashObject` stable across the two ends.
   */
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
 */
export function recordLockfileVerified (opts: RecordLockfileVerifiedOptions): void {
  if (!opts.cacheDir) return
  if (!opts.resolutionVerifiers?.length) return
  if (!opts.lockfile.packages) return
  recordVerification(opts.cacheDir, {
    lockfilePath: opts.lockfilePath,
    verifiers: opts.resolutionVerifiers,
    hashLockfile: () => hashObject(opts.lockfile),
  })
}
