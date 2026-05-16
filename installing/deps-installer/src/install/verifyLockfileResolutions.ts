import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { ActiveVerifier, ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'
import pLimit from 'p-limit'

import {
  recordVerification,
  tryLockfileVerificationCache,
} from './verifyLockfileResolutionsCache.js'

interface Violation {
  pkgId: string
  code: string
  reason: string
}

// Cap the per-entry breakdown so a verifier rejecting hundreds of entries
// (e.g. a poisoned lockfile) doesn't flood the terminal / CI log; the full
// count is in the header and the remainder is summarized at the end.
const MAX_VIOLATIONS_TO_PRINT = 20

// 16 mirrors the floor of pnpm's package-requester network-concurrency
// (Math.min(64, Math.max(workers*3, 16))); keep them aligned so the
// verification pass doesn't push past what the rest of the install respects.
const DEFAULT_CONCURRENCY = 16

export interface VerifyLockfileResolutionsCacheOptions {
  /** pnpm's on-disk cache directory. The lockfile-verification cache file lives directly under it. */
  cacheDir: string
  /** Absolute path of the lockfile being verified — the cache key. */
  lockfilePath: string
  /**
   * Policy snapshots for every verifier whose result this lookup/record
   * covers. Each verifier owns a stable key (e.g. `npm.minimumReleaseAge`)
   * and a `satisfies` comparator that decides whether a cached snapshot
   * still meets today's policy. The cache only short-circuits when **all**
   * active verifiers agree the cached run is still valid.
   */
  verifiers: readonly ActiveVerifier[]
}

/**
 * Policy-neutral pass that asks each resolver-supplied {@link ResolutionVerifier}
 * to check every entry in a lockfile loaded from disk. Iteration runs
 * before resolution decisions are touched and before any tarball is
 * fetched, so a lockfile whose entries were resolved elsewhere (committed
 * to the repo, restored from a cache, etc.) under a weaker or absent
 * policy cannot reach the filesystem. Fresh local resolution is covered
 * by the resolver's own per-version filter.
 *
 * Designed for fail-closed semantics at the verifier level: a verifier that
 * can't confirm a resolution is expected to return `{ ok: false }` rather
 * than passing silently — otherwise a registry hiccup or an unpublished
 * version would re-open the bypass.
 *
 * No-op when `verifyResolution` is undefined (no active policies).
 *
 * When `options.cache` is provided, an unchanged lockfile that has already
 * been verified under the same (or stricter) policy short-circuits the
 * registry round-trip entirely — see {@link tryLockfileVerificationCache}
 * for the lookup logic.
 */
export async function verifyLockfileResolutions (
  lockfile: LockfileObject,
  verifyResolution: ResolutionVerifier | undefined,
  options?: { concurrency?: number, cache?: VerifyLockfileResolutionsCacheOptions }
): Promise<void> {
  if (verifyResolution == null) return
  if (!lockfile.packages) return

  // Cache lookup runs before any registry I/O — the fast path is a single
  // stat() of the lockfile when the previous install already verified it
  // under a policy that's at least as strict as today's.
  if (options?.cache && options.cache.verifiers.length > 0) {
    const { hit } = await tryLockfileVerificationCache(options.cache.cacheDir, {
      lockfilePath: options.cache.lockfilePath,
      verifiers: options.cache.verifiers,
    })
    if (hit) return
  }

  // depPath can include peer-dependency and patch_hash suffixes (e.g.
  // `react@18.0.0(peer)(patch_hash=…)`); the same (name, version) pair may
  // therefore appear multiple times. Dedupe so we issue at most one
  // verification per package version.
  const candidates = new Map<string, { name: string, version: string, resolution: unknown }>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !version) continue
    candidates.set(`${name}@${version}`, { name, version, resolution: snapshot.resolution })
  }

  const violations: Violation[] = []
  const limit = pLimit(options?.concurrency ?? DEFAULT_CONCURRENCY)
  await Promise.all(
    Array.from(candidates.values(), ({ name, version, resolution }) => limit(async () => {
      const pkgId = `${name}@${version}`
      const result = await verifyResolution(resolution as Parameters<ResolutionVerifier>[0], { name, version })
      if (!result.ok) {
        violations.push({ pkgId, code: result.code, reason: result.reason })
      }
    }))
  )

  if (violations.length === 0) {
    // Persist the success so the next install can stat-only the lockfile.
    if (options?.cache && options.cache.verifiers.length > 0) {
      await recordVerification(options.cache.cacheDir, {
        lockfilePath: options.cache.lockfilePath,
        verifiers: options.cache.verifiers,
      })
    }
    return
  }

  // Stable order so the error output is deterministic.
  violations.sort((a, b) => a.pkgId.localeCompare(b.pkgId))
  const visible = violations.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = violations.length - visible.length
  const breakdown = visible.map((v) => `  ${v.pkgId} ${v.reason}`).join('\n')
  const details = omitted > 0
    ? `${breakdown}\n  …and ${omitted} more`
    : breakdown
  // Use the code of the first violation — all of today's violations are the
  // same shape (one verifier, one code). If multiple verifiers fire later
  // with mixed codes, switch to a generic LOCKFILE_RESOLUTION_VERIFICATION
  // code and list per-entry codes in the breakdown.
  throw new PnpmError(
    violations[0].code,
    `${violations.length} lockfile entries failed verification:\n${details}`,
    {
      hint: 'The lockfile contains entries that the active policies reject. ' +
        'This can mean the lockfile is stale, or that someone committed a ' +
        'lockfile that bypassed the policy locally — inspect recent changes ' +
        'to pnpm-lock.yaml before trusting it. If the changes look expected, ' +
        'run "pnpm clean --lockfile" and then "pnpm install" to rebuild from ' +
        'a fresh resolution. Alternatively, relax the policy that flagged ' +
        'them.',
    }
  )
}
