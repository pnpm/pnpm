import { hashObject } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { Resolution, ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'
import pLimit from 'p-limit'

import {
  recordVerification,
  tryLockfileVerificationCache,
} from './verifyLockfileResolutionsCache.js'

/**
 * One verifier outcome against one lockfile entry. The shape is
 * resolver-agnostic — every verifier reports its own `code` (e.g.
 * `MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`) and the caller
 * decides how to react (throw, prompt, persist, log).
 *
 * Exposed so other resolvers (jsr, git, custom) can have their
 * verifiers participate in the same collect-mode scan the install
 * uses for loose-mode auto-collect and the strict-mode prompt — no
 * minimumReleaseAge-specific plumbing required.
 */
export interface LockfileResolutionViolation {
  name: string
  version: string
  /** Resolution from the lockfile that the verifier rejected. */
  resolution: Resolution
  /** Verifier-defined code. Drives downstream UX (which exclude list to populate, etc.). */
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

export interface VerifyLockfileResolutionsOptions {
  concurrency?: number
  /**
   * pnpm's on-disk cache directory. When set together with
   * `lockfilePath`, verification results are memoized in
   * `<cacheDir>/lockfile-verified.jsonl` and the gate short-circuits on
   * a repeat run against an unchanged lockfile + same-or-stricter
   * policy. Omit to disable the cache entirely (every call rehashes
   * the lockfile and re-verifies).
   */
  cacheDir?: string
  /** Absolute path of the lockfile being verified. Used by the cache's stat shortcut. */
  lockfilePath?: string
}

/**
 * Policy-neutral pass that asks every resolver-supplied
 * {@link ResolutionVerifier} to check every entry in a lockfile loaded
 * from disk. Iteration runs before resolution decisions are touched and
 * before any tarball is fetched, so a lockfile whose entries were
 * resolved elsewhere (committed to the repo, restored from a cache,
 * etc.) under a weaker or absent policy cannot reach the filesystem.
 * Fresh local resolution is covered by the resolver's own per-version
 * filter.
 *
 * Each verifier handles its own protocol short-circuit inside `verify`
 * (returning `{ ok: true }` for resolutions outside its scope), so the
 * fan-out is policy-neutral and dispatch-free at this layer.
 *
 * Designed for fail-closed semantics at the verifier level: a verifier
 * that can't confirm a resolution is expected to return `{ ok: false }`
 * rather than passing silently — otherwise a registry hiccup or an
 * unpublished version would re-open the bypass.
 *
 * No-op when `verifiers` is empty.
 *
 * When `options.cacheDir` and `options.lockfilePath` are both
 * provided, an unchanged lockfile that has already been verified
 * under the same (or stricter) policy short-circuits the registry
 * round-trip entirely — see {@link tryLockfileVerificationCache} for
 * the lookup logic.
 */
export async function verifyLockfileResolutions (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options?: VerifyLockfileResolutionsOptions
): Promise<void> {
  if (verifiers.length === 0) return
  if (!lockfile.packages) return

  // Caching kicks in only when the caller surfaced both a writable
  // cache directory and the lockfile's absolute path — that's the
  // production wiring; unit tests that skip them get the gate without
  // memoization and still exercise the same code path.
  const cache = options?.cacheDir && options?.lockfilePath
    ? { cacheDir: options.cacheDir, lockfilePath: options.lockfilePath }
    : undefined

  // Cache lookup runs before any registry I/O — the fast path is a
  // single stat() of the lockfile when the previous install already
  // verified it under a policy that's at least as strict as today's.
  // The content key is hashed lazily from the in-memory lockfile (not
  // the file bytes) so we never read the file a second time. On a
  // miss the precomputed stat+hash flow to recordVerification.
  type Precomputed = ReturnType<typeof tryLockfileVerificationCache>['precomputed']
  let cachePrecomputed: Precomputed | undefined
  // hashObject is streaming (no "Invalid string length" on huge lockfiles)
  // and key-order-stable, which JSON.stringify is not.
  let cachedHash: string | undefined
  const hashLockfile = (): string => {
    if (cachedHash == null) cachedHash = hashObject(lockfile)
    return cachedHash
  }
  if (cache) {
    const result = tryLockfileVerificationCache(cache.cacheDir, {
      lockfilePath: cache.lockfilePath,
      verifiers,
      hashLockfile,
    })
    if (result.hit) return
    cachePrecomputed = result.precomputed
  }

  const violations = await iterateLockfileViolations(lockfile, verifiers, options?.concurrency)

  if (violations.length === 0) {
    // Persist the success so the next install can stat-only the lockfile.
    if (cache) {
      recordVerification(cache.cacheDir, {
        lockfilePath: cache.lockfilePath,
        verifiers,
        hashLockfile,
      }, cachePrecomputed)
    }
    return
  }

  // Stable order so the error output is deterministic.
  violations.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  const visible = violations.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = violations.length - visible.length
  const breakdown = visible.map((v) => `  ${v.name}@${v.version} ${v.reason}`).join('\n')
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

/**
 * Collect-mode sibling of {@link verifyLockfileResolutions}: runs the
 * same fan-out over every verifier and every lockfile entry, but
 * returns the violations as data instead of throwing on the first batch.
 * No cache lookup or write — the throw-mode `verifyLockfileResolutions`
 * is what populates / honors the cache; this is for callers that need
 * to inspect violations (auto-collect into `minimumReleaseAgeExclude`,
 * the strict-mode interactive prompt, future resolver-specific
 * policies).
 *
 * Returns an empty array when `verifiers` is empty or the lockfile has
 * no packages, so callers don't need a separate emptiness check.
 */
export async function collectLockfileResolutionViolations (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options?: Pick<VerifyLockfileResolutionsOptions, 'concurrency'>
): Promise<LockfileResolutionViolation[]> {
  if (verifiers.length === 0) return []
  if (!lockfile.packages) return []
  return iterateLockfileViolations(lockfile, verifiers, options?.concurrency)
}

async function iterateLockfileViolations (
  lockfile: LockfileObject,
  verifiers: readonly ResolutionVerifier[],
  concurrency: number | undefined
): Promise<LockfileResolutionViolation[]> {
  // depPath can include peer-dependency and patch_hash suffixes (e.g.
  // `react@18.0.0(peer)(patch_hash=…)`); the same (name, version) pair may
  // therefore appear multiple times. Dedupe so we issue at most one
  // verification per package version.
  //
  // Include a serialization of `resolution` in the key so two entries that
  // share a (name, version) but differ in *what* was resolved (e.g. one
  // pinned via npm, another via a git URL under the same alias) don't
  // collapse: if the wrong shape wins the dedup, a protocol-scoped
  // verifier short-circuits on the surviving entry and the real one is
  // never checked.
  const candidates = new Map<string, { name: string, version: string, resolution: Resolution }>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !version) continue
    const key = `${name}@${version}@${JSON.stringify(snapshot.resolution)}`
    candidates.set(key, {
      name,
      version,
      resolution: snapshot.resolution as Resolution,
    })
  }

  const violations: LockfileResolutionViolation[] = []
  const limit = pLimit(concurrency ?? DEFAULT_CONCURRENCY)
  await Promise.all(
    Array.from(candidates.values(), ({ name, version, resolution }) => limit(async () => {
      // Fan out across every active verifier; each handles its own
      // protocol short-circuit (e.g. the npm verifier returns ok:true for
      // git resolutions). We stop at the first failure per entry so a
      // multi-verifier setup doesn't produce duplicate violations for the
      // same (name, version).
      for (const verifier of verifiers) {
        // eslint-disable-next-line no-await-in-loop
        const result = await verifier.verify(resolution, { name, version })
        if (!result.ok) {
          violations.push({ name, version, resolution, code: result.code, reason: result.reason })
          break
        }
      }
    }))
  )
  return violations
}
