import { lockfileVerificationLogger } from '@pnpm/core-loggers'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { parse } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type {
  Resolution,
  ResolutionPolicyViolation,
  ResolutionVerifier,
} from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'
import pLimit from 'p-limit'

import {
  recordVerification,
  tryLockfileVerificationCache,
} from './verifyLockfileResolutionsCache.js'

// Re-exported for back-compat with the existing import surface.
// The interface itself lives in resolver-base so deps-resolver can
// participate in the same shape; see the doc there.
export type { ResolutionPolicyViolation }

// Cap the per-entry breakdown so a verifier rejecting hundreds of entries
// (e.g. a poisoned lockfile) doesn't flood the terminal / CI log; the full
// count is in the header and the remainder is summarized at the end.
const MAX_VIOLATIONS_TO_PRINT = 20

// 16 mirrors the floor of pnpm's package-requester network-concurrency
// (Math.min(64, Math.max(workers*3, 16))); keep them aligned so the
// verification pass doesn't push past what the rest of the install respects.
const DEFAULT_CONCURRENCY = 16

export const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE = 'RESOLUTION_SHAPE_MISMATCH'

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
  if (!lockfile.packages) return

  // The structural pass is offline and runs unconditionally — even with no
  // policy verifiers active and regardless of the verification cache. The
  // allowBuilds policy treats a registry-style depPath (name@semver) as a
  // trusted package identity, which is only sound while this pass rejects
  // lockfiles where such a key is backed by a non-registry resolution.
  const shapeViolations = collectResolutionShapeViolations(lockfile)
  if (shapeViolations.length > 0) {
    throw buildVerificationError(shapeViolations)
  }

  if (verifiers.length === 0) return

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
  // hashObject streams and is key-order-stable, unlike JSON.stringify.
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

  // Emit started/done around the actual verification pass — the
  // round-trip can be slow on a cold registry cache, and the cached
  // short-circuit above doesn't reach this branch, so a user only
  // sees these messages on installs that are doing real work.
  // A degenerate lockfile where every snapshot fails the
  // name/version extraction (so candidates is empty) skips emission
  // entirely — no work, no noise.
  const candidates = collectCandidates(lockfile)
  if (candidates.size === 0) {
    if (cache) {
      recordVerification(cache.cacheDir, {
        lockfilePath: cache.lockfilePath,
        verifiers,
        hashLockfile,
      }, cachePrecomputed)
    }
    return
  }
  const startedAt = Date.now()
  lockfileVerificationLogger.debug({
    status: 'started',
    entries: candidates.size,
    lockfilePath: options?.lockfilePath,
  })
  // Guarantee a terminal `done` or `failed` event on every exit path
  // that emitted `started`. Without this, an unexpected throw from the
  // registry fan-out (or the policy-violation throw below) would leave
  // the transient "Verifying lockfile…" line as the last frame the
  // reporter rendered for this block, hanging spinner-style above the
  // failure output.
  let terminalStatus: 'done' | 'failed' = 'failed'
  try {
    const violations = await iterateLockfileViolations(candidates, verifiers, options?.concurrency)
    if (violations.length === 0) {
      terminalStatus = 'done'
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
    throw buildVerificationError(violations)
  } finally {
    lockfileVerificationLogger.debug({
      status: terminalStatus,
      entries: candidates.size,
      elapsedMs: Date.now() - startedAt,
      lockfilePath: options?.lockfilePath,
    })
  }
}

function buildVerificationError (violations: ResolutionPolicyViolation[]): PnpmError {
  // Stable order so the error output is deterministic.
  violations.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  // Pick the throw code: a single-code batch keeps the per-policy code
  // (so existing handlers / docs / search keywords still route correctly);
  // a mixed batch (e.g. minimumReleaseAge + trust-downgrade on the same
  // lockfile) escalates to the generic `LOCKFILE_RESOLUTION_VERIFICATION`
  // and the per-entry code goes into the breakdown so the user can see
  // which policy each entry tripped.
  const distinctCodes = new Set(violations.map((v) => v.code))
  const isMixed = distinctCodes.size > 1
  const errorCode = isMixed ? 'LOCKFILE_RESOLUTION_VERIFICATION' : violations[0].code
  const visible = violations.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = violations.length - visible.length
  const formatEntry = isMixed
    ? (v: ResolutionPolicyViolation): string => `  ${v.name}@${v.version} [${v.code}] ${v.reason}`
    : (v: ResolutionPolicyViolation): string => `  ${v.name}@${v.version} ${v.reason}`
  const breakdown = visible.map(formatEntry).join('\n')
  const details = omitted > 0
    ? `${breakdown}\n  …and ${omitted} more`
    : breakdown
  return new PnpmError(
    errorCode,
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
export async function collectResolutionPolicyViolations (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options?: Pick<VerifyLockfileResolutionsOptions, 'concurrency'>
): Promise<ResolutionPolicyViolation[]> {
  if (verifiers.length === 0) return []
  if (!lockfile.packages) return []
  return iterateLockfileViolations(collectCandidates(lockfile), verifiers, options?.concurrency)
}

function collectResolutionShapeViolations (lockfile: LockfileObject): ResolutionPolicyViolation[] {
  const violations: ResolutionPolicyViolation[] = []
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion } = parse(depPath)
    if (name == null || version == null || nonSemverVersion != null) continue
    if (isRegistryShapedResolution(snapshot.resolution)) continue
    violations.push({
      name,
      version,
      resolution: snapshot.resolution as Resolution,
      code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
      reason: 'a registry-style dependency path is backed by a non-registry resolution',
    })
  }
  return violations
}

function isRegistryShapedResolution (resolution: unknown): boolean {
  if (resolution == null) return true
  if (typeof resolution !== 'object') return false
  const { type, gitHosted, variants } = resolution as { type?: unknown, gitHosted?: unknown, variants?: unknown }
  if (type === 'variations') {
    return Array.isArray(variants) && variants.every(
      (variant) => isRegistryShapedResolution((variant as { resolution?: unknown })?.resolution)
    )
  }
  if (type != null) return false
  if (gitHosted === true) return false
  return true
}

interface Candidate {
  name: string
  version: string
  nonSemverVersion?: string
  resolution: Resolution
}

// depPath can include peer-dependency and patch_hash suffixes (e.g.
// `react@18.0.0(peer)(patch_hash=…)`); the same (name, version) pair may
// therefore appear multiple times. Dedupe so we issue at most one
// verification per package version.
//
// Include a serialization of `resolution` and `nonSemverVersion` in the key
// so two entries that share a (name, version) but differ in *what* was
// resolved (e.g. one pinned via npm, another a URL-keyed tarball whose
// snapshot copied the same semver `version` from its manifest) don't
// collapse: `nonSemverVersion` flips whether the npm verifier enforces or
// skips the tarball/policy checks, so if the wrong shape wins the dedup the
// surviving entry is verified under the wrong rules and the real one is
// never checked.
function collectCandidates (lockfile: LockfileObject): Map<string, Candidate> {
  const candidates = new Map<string, Candidate>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !version) continue
    const key = `${name}@${version}@${nonSemverVersion ?? ''}@${JSON.stringify(snapshot.resolution)}`
    candidates.set(key, {
      name,
      version,
      nonSemverVersion,
      resolution: snapshot.resolution as Resolution,
    })
  }
  return candidates
}

async function iterateLockfileViolations (
  candidates: Map<string, Candidate>,
  verifiers: readonly ResolutionVerifier[],
  concurrency: number | undefined
): Promise<ResolutionPolicyViolation[]> {
  const violations: ResolutionPolicyViolation[] = []
  const limit = pLimit(concurrency ?? DEFAULT_CONCURRENCY)
  await Promise.all(
    Array.from(candidates.values(), ({ name, version, nonSemverVersion, resolution }) => limit(async () => {
      // Fan out across every active verifier; each handles its own
      // protocol short-circuit (e.g. the npm verifier returns ok:true for
      // git resolutions). We stop at the first failure per entry so a
      // multi-verifier setup doesn't produce duplicate violations for the
      // same (name, version).
      for (const verifier of verifiers) {
        // eslint-disable-next-line no-await-in-loop
        const result = await verifier.verify(resolution, { name, version, nonSemverVersion })
        if (!result.ok) {
          violations.push({ name, version, resolution, code: result.code, reason: result.reason })
          break
        }
      }
    }))
  )
  return violations
}
