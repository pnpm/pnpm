import { lockfileVerificationLogger } from '@pnpm/core-loggers'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { isValidDependencyAlias } from '@pnpm/installing.deps-resolver'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { isGitHostedTarballUrl, nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
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
  type VerifierCacheIdentity,
} from './verifyLockfileResolutionsCache.js'

// Re-exported for back-compat with the existing import surface.
// The interface itself lives in resolver-base so deps-resolver can
// participate in the same shape; see the doc there.
export type { ResolutionPolicyViolation }

// Cap the per-entry breakdown so a verifier rejecting hundreds of entries
// (e.g. a poisoned lockfile) doesn't flood the terminal / CI log; the full
// count is in the header and the remainder is summarized at the end.
const MAX_VIOLATIONS_TO_PRINT = 20

// 64 mirrors the floor of pnpm's package-requester network-concurrency
// (Math.min(96, Math.max(workers*3, 64))); keep them aligned so the
// verification pass doesn't push past what the rest of the install respects.
const DEFAULT_CONCURRENCY = 64

export const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE = 'RESOLUTION_SHAPE_MISMATCH'

// Same code the sink-level guards (`safeJoinModulesDir`) throw.
export const INVALID_DEPENDENCY_ALIAS_CODE = 'INVALID_DEPENDENCY_NAME'

const RESOLUTION_SHAPE_CACHE_IDENTITY: VerifierCacheIdentity = {
  policy: { resolutionShapeCheck: true },
  canTrustPastCheck: (cached) => cached.resolutionShapeCheck === true,
}

const DEPENDENCY_ALIAS_CACHE_IDENTITY: VerifierCacheIdentity = {
  policy: { dependencyAliasCheck: true },
  canTrustPastCheck: (cached) => cached.dependencyAliasCheck === true,
}

/**
 * Every verifier list that flows into the verification cache must carry
 * the always-on offline structural checks' identities, so a record
 * written before one of those rules existed cannot stat-fast-path around
 * it — its missing flag fails `canTrustPastCheck`, forcing a
 * re-verification that runs the new check. Used by the gate itself and by
 * {@link recordLockfileVerified}, whose freshly-resolved lockfile
 * satisfies these invariants by construction (the resolver validates
 * aliases at manifest-read time and derives every resolution key from the
 * resolution it just produced).
 */
export function withOfflineCheckCacheIdentities (verifiers: readonly VerifierCacheIdentity[]): VerifierCacheIdentity[] {
  return [...verifiers, RESOLUTION_SHAPE_CACHE_IDENTITY, DEPENDENCY_ALIAS_CACHE_IDENTITY]
}

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

  // Caching kicks in only when the caller surfaced both a writable
  // cache directory and the lockfile's absolute path — that's the
  // production wiring; unit tests that skip them get the gate without
  // memoization and still exercise the same code path.
  const cache = options?.cacheDir && options?.lockfilePath
    ? { cacheDir: options.cacheDir, lockfilePath: options.lockfilePath }
    : undefined

  const cacheVerifiers = withOfflineCheckCacheIdentities(verifiers)

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
      verifiers: cacheVerifiers,
      hashLockfile,
    })
    if (result.hit) {
      // A silent short-circuit looks like the policy gate never ran
      // (pnpm/pnpm#12324), so surface the reused verdict — but only
      // when policy verifiers are active; the shape-only run that
      // every install performs stays quiet.
      if (verifiers.length > 0) {
        lockfileVerificationLogger.debug({
          status: 'cached',
          verifiedAt: result.verifiedAt,
          lockfilePath: options?.lockfilePath,
        })
      }
      return
    }
    cachePrecomputed = result.precomputed
  }

  // Emit started/done around the actual verification pass — the
  // round-trip can be slow on a cold registry cache, and the cached
  // short-circuit above announces itself with its own `cached` event,
  // so a user only sees these messages on installs that are doing
  // real work.
  // A degenerate lockfile where every snapshot fails the
  // name/version extraction (so candidates is empty) skips emission
  // entirely — no work, no noise.
  const { candidates, shapeViolations, invalidAliases } = collectCandidates(lockfile)
  if (invalidAliases.length > 0) {
    throw buildInvalidAliasError(invalidAliases)
  }
  if (shapeViolations.length > 0) {
    throw buildVerificationError(shapeViolations)
  }
  if (verifiers.length === 0) return
  if (candidates.size === 0) {
    if (cache) {
      recordVerification(cache.cacheDir, {
        lockfilePath: cache.lockfilePath,
        verifiers: cacheVerifiers,
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
          verifiers: cacheVerifiers,
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

function buildInvalidAliasError (aliases: string[]): PnpmError {
  const sorted = [...aliases].sort()
  const visible = sorted.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = sorted.length - visible.length
  const breakdown = visible.map((alias) => `  ${JSON.stringify(alias)}`).join('\n')
  const details = omitted > 0 ? `${breakdown}\n  …and ${omitted} more` : breakdown
  const plural = aliases.length === 1 ? 'alias' : 'aliases'
  return new PnpmError(
    INVALID_DEPENDENCY_ALIAS_CODE,
    `The lockfile contains ${aliases.length} dependency ${plural} that are not valid package names:\n${details}`,
    {
      hint: 'A dependency alias becomes a directory under node_modules, so it must be a valid npm package name — a single `name` or `@scope/name` with no leading `.` or `_`, and not a reserved name such as `node_modules`. ' +
        'An alias containing path-traversal segments or a reserved name such as `.bin` or `.pnpm` could make an install write outside the intended directory or overwrite pnpm-owned layout. ' +
        'This usually means the lockfile was tampered with — inspect recent changes to pnpm-lock.yaml before trusting it.',
    }
  )
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
  // Registry fetch failures (auth/network/5xx) don't reach this batch — the
  // verifier throws the registry's own error and the gate aborts with it. So
  // every violation here is a genuine policy rejection, and the hint points at
  // the lockfile rather than at connectivity.
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
  // Shape violations are deliberately not collected here: they are hard
  // tampering failures, not policy picks a caller may auto-exclude.
  return iterateLockfileViolations(collectCandidates(lockfile).candidates, verifiers, options?.concurrency)
}

function isRegistryShapedResolution (resolution: unknown): boolean {
  if (resolution == null) return true
  if (typeof resolution !== 'object') return false
  const { type, gitHosted, tarball, variants } = resolution as {
    type?: unknown
    gitHosted?: unknown
    tarball?: unknown
    variants?: unknown
  }
  if (type === 'variations') {
    return Array.isArray(variants) && variants.every(
      (variant) => isRegistryShapedResolution((variant as { resolution?: unknown })?.resolution)
    )
  }
  // Custom resolver protocols (`type: 'custom:*'`) are a legitimate
  // non-registry source the user opted into. They can only be materialized by
  // a project-configured custom fetcher — an unrecognized custom type throws at
  // fetch time (see @pnpm/fetching.pick-fetcher) — so a forged custom type
  // cannot launder an artifact past this gate into a build.
  if (typeof type === 'string' && type.startsWith('custom:')) return true
  if (type != null) return false
  // Plain tarball / registry resolution. The lockfile is parsed from YAML
  // without schema validation, so the `gitHosted` flag is not trustworthy on
  // its own: a tampered entry could set a non-boolean (dodging a strict
  // `=== true`) or an explicit `false` on a git-host URL (the loader only
  // backfills the flag when absent). Treat any non-boolean flag as git-hosted
  // and gate on the URL so the verdict never depends on the flag alone.
  if (gitHosted != null && (typeof gitHosted !== 'boolean' || gitHosted)) return false
  // A registry resolution reconstructs its tarball URL from name+version, so
  // an absent/empty `tarball` is registry-shaped. When a URL is present it
  // must be an http(s) registry artifact: the npm verifier's tarball-URL
  // binding skips non-http(s) schemes (file:, etc.), so a `file:` tarball
  // under a name@semver key would otherwise be trusted with no safety net.
  if (typeof tarball === 'string' && tarball !== '') {
    if (!/^https?:\/\//i.test(tarball)) return false
    if (isGitHostedTarballUrl(tarball)) return false
  }
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
function collectCandidates (lockfile: LockfileObject): { candidates: Map<string, Candidate>, shapeViolations: ResolutionPolicyViolation[], invalidAliases: string[] } {
  const candidates = new Map<string, Candidate>()
  const shapeViolations: ResolutionPolicyViolation[] = []
  // The importer alias maps are the one source not reached by the
  // package loop below, so they're scanned here.
  const invalidAliases = new Set<string>()
  for (const importer of Object.values(lockfile.importers ?? {})) {
    pushInvalidAliases(importer.dependencies, invalidAliases)
    pushInvalidAliases(importer.devDependencies, invalidAliases)
    pushInvalidAliases(importer.optionalDependencies, invalidAliases)
  }
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    pushInvalidAliases(snapshot.dependencies, invalidAliases)
    pushInvalidAliases(snapshot.optionalDependencies, invalidAliases)
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !version) continue
    // A registry-style depPath (name@semver) must be backed by a
    // registry-shaped resolution: the allowBuilds policy derives a
    // trusted package identity from that key shape, which is only sound
    // while this invariant holds. The check is offline, so it applies
    // even when no policy verifiers are active.
    if (nonSemverVersion == null && !isRegistryShapedResolution(snapshot.resolution)) {
      shapeViolations.push({
        name,
        version,
        resolution: snapshot.resolution as Resolution,
        code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
        reason: 'a registry-style dependency path is backed by a non-registry resolution',
      })
    }
    const key = `${name}@${version}@${nonSemverVersion ?? ''}@${JSON.stringify(snapshot.resolution)}`
    candidates.set(key, {
      name,
      version,
      nonSemverVersion,
      resolution: snapshot.resolution as Resolution,
    })
  }
  return { candidates, shapeViolations, invalidAliases: Array.from(invalidAliases) }
}

/**
 * Add every key of `deps` that is not a valid {@link isValidDependencyAlias}
 * to `invalid`. Only pass maps whose keys become `node_modules/<alias>`
 * directories — not `overrides` (`foo>bar` selectors) or
 * `patchedDependencies` (`name@version` keys).
 */
function pushInvalidAliases (deps: Record<string, string> | undefined, invalid: Set<string>): void {
  if (deps == null) return
  for (const alias of Object.keys(deps)) {
    if (!isValidDependencyAlias(alias)) invalid.add(alias)
  }
}

async function iterateLockfileViolations (
  candidates: Map<string, Candidate>,
  verifiers: readonly ResolutionVerifier[],
  concurrency: number | undefined
): Promise<ResolutionPolicyViolation[]> {
  const violations: ResolutionPolicyViolation[] = []
  // A verifier may throw rather than return a violation when it can't reach the
  // registry to verify an entry (auth/network/5xx) — that's not a per-entry
  // policy pick, it's an incomplete verification, so the registry's own error
  // should abort the install. Capture the first such error and rethrow it after
  // the fan-out settles: rethrowing straight into Promise.all would leave the
  // sibling tasks (all failing against the same dead registry) as unhandled
  // rejections once Promise.all rejects on the first.
  let fetchError: unknown
  const limit = pLimit(concurrency ?? DEFAULT_CONCURRENCY)
  await Promise.all(
    Array.from(candidates.values(), ({ name, version, nonSemverVersion, resolution }) => limit(async () => {
      try {
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
      } catch (err) {
        fetchError ??= err
      }
    }))
  )
  // A registry that couldn't be reached takes precedence over collected
  // violations: the run never finished verifying, so the batch is incomplete
  // and the actionable failure is the transport error. Once it's resolved the
  // re-run surfaces any remaining violations.
  if (fetchError != null) throw fetchError
  return violations
}
