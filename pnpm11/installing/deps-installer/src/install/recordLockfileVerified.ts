import { hashObject } from '@pnpm/crypto.object-hasher'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import {
  type ExoticSubdepsScope,
  hasInvalidDependencyAliases,
  withOfflineCheckCacheIdentities,
} from './verifyLockfileResolutions.js'
import { recordVerificationByHash } from './verifyLockfileResolutionsCache.js'

export interface RecordLockfileVerifiedOptions {
  cacheDir?: string
  /** The writer's canonical return value — see {@link writeWantedLockfile}.
   *  Passing the raw in-memory write object would record a hash the
   *  next install can't match (YAML drops undefined fields). */
  lockfile: LockfileObject
  resolutionVerifiers: readonly ResolutionVerifier[] | undefined
  blockExoticSubdeps?: boolean
  exoticSubdepsScope?: ExoticSubdepsScope
}

/**
 * Records the post-resolution lockfile as verified so the next install
 * skips the registry round-trip. Skipping is safe: fresh local picks
 * are filtered by the resolver (see
 * `resolving/npm-resolver/src/pickPackage.ts`) and carried-over entries
 * already passed the gate at the top of `mutateModules`, so the
 * recorded lockfile is policy-clean by construction.
 *
 * Recorded by content hash only. A stat of the lockfile path taken after
 * the write could describe a file another process put there since, and a
 * later stat-cache hit would skip checking it. The next install hashes the
 * lockfile it read, finds this record, and indexes its own stat.
 */
export function recordLockfileVerified (opts: RecordLockfileVerifiedOptions): void {
  if (!opts.cacheDir) return
  if (!opts.resolutionVerifiers?.length && !opts.blockExoticSubdeps) return
  if (!opts.lockfile.packages) return
  if (hasInvalidDependencyAliases(opts.lockfile)) return
  recordVerificationByHash(opts.cacheDir, {
    verifiers: withOfflineCheckCacheIdentities(opts.resolutionVerifiers ?? [], {
      blockExoticSubdeps: opts.blockExoticSubdeps,
      exoticSubdepsScope: opts.exoticSubdepsScope,
    }),
    hashLockfile: () => hashObject(opts.lockfile),
  })
}
