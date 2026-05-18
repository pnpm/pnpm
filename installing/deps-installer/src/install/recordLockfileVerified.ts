import { hashObject } from '@pnpm/crypto.object-hasher'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordVerification } from './verifyLockfileResolutionsCache.js'

export interface RecordLockfileVerifiedOptions {
  cacheDir?: string
  /** Absolute path of the lockfile the next install will read.
   *  Under `useGitBranchLockfile` this is the branch-suffixed name. */
  lockfilePath: string
  /** The writer's canonical return value — see {@link writeWantedLockfile}.
   *  Passing the raw in-memory write object would record a hash the
   *  next install can't match (YAML drops undefined fields). */
  lockfile: LockfileObject
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
}

/**
 * Records a post-resolution lockfile in the verification cache so the
 * next install with an unchanged lockfile takes the stat/hash fast path
 * instead of re-checking every entry against the registry.
 *
 * Safe to call because the lockfile is policy-clean by construction:
 * fresh local resolution passes through the resolver's per-version
 * filter (see `resolving/npm-resolver/src/pickPackage.ts`), and any
 * carried-over entries already passed the gate at the top of
 * `mutateModules`.
 *
 * No-op when the cache isn't wired, no verifiers are active, or the
 * lockfile has no packages — same gating as
 * {@link verifyLockfileResolutions}.
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
