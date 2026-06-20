import { hashObject } from '@pnpm/crypto.object-hasher'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { withOfflineCheckCacheIdentities } from './verifyLockfileResolutions.js'
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
 * Records the post-resolution lockfile as verified so the next install
 * skips the registry round-trip. Skipping is safe: fresh local picks
 * are filtered by the resolver (see
 * `resolving/npm-resolver/src/pickPackage.ts`) and carried-over entries
 * already passed the gate at the top of `mutateModules`, so the
 * recorded lockfile is policy-clean by construction.
 */
export function recordLockfileVerified (opts: RecordLockfileVerifiedOptions): void {
  if (!opts.cacheDir) return
  if (!opts.resolutionVerifiers?.length) return
  if (!opts.lockfile.packages) return
  recordVerification(opts.cacheDir, {
    lockfilePath: opts.lockfilePath,
    verifiers: withOfflineCheckCacheIdentities(opts.resolutionVerifiers),
    hashLockfile: () => hashObject(opts.lockfile),
  })
}
