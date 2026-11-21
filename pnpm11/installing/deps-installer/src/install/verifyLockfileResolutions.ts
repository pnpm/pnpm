import path from 'node:path'

import { lockfileVerificationLogger } from '@pnpm/core-loggers'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE, TRUST_DOWNGRADE_VIOLATION_CODE } from '@pnpm/resolving.npm-resolver'
import type {
  Resolution,
  ResolutionPolicyViolation,
  ResolutionVerifier,
} from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'
import pLimit from 'p-limit'

import { withOfflineCheckCacheIdentities } from './verifyLockfileCacheIdentities.js'
import {
  buildExoticSubdepsScopeFromContext,
  collectExoticSubdepViolations,
  EXOTIC_SUBDEP_VIOLATION_CODE,
  type ExoticSubdepsScope,
} from './verifyLockfileExoticSubdeps.js'
import {
  collectLocalResolutionViolations,
  LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
  type LocalResolutionPolicy,
} from './verifyLockfileLocalResolutions.js'
import {
  collectInvalidAliases,
  collectLockfileFileOfflineViolations,
  collectShapeViolations,
  hasInvalidDependencyAliases,
  RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
} from './verifyLockfileOfflineChecks.js'
import {
  lookupVerificationCache,
  persistVerificationCache,
  type tryLockfileVerificationCache,
  type VerifierCacheIdentity,
} from './verifyLockfileResolutionsCache.js'
import type { FileShapeContext } from './verifyLockfileShape.js'

export {
  buildExoticSubdepsScopeFromContext,
  EXOTIC_SUBDEP_VIOLATION_CODE,
  type ExoticSubdepsScope,
  hasInvalidDependencyAliases,
  LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
  type LocalResolutionPolicy,
  RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
}

export type { ResolutionPolicyViolation }

const MAX_VIOLATIONS_TO_PRINT = 20
const DEFAULT_CONCURRENCY = 64

const POLICY_VIOLATION_CODES = new Set([
  MINIMUM_RELEASE_AGE_VIOLATION_CODE,
  TRUST_DOWNGRADE_VIOLATION_CODE,
  EXOTIC_SUBDEP_VIOLATION_CODE,
])

export const INVALID_DEPENDENCY_ALIAS_CODE = 'INVALID_DEPENDENCY_NAME'
export {
  canTrustPreparedPolicy,
  RESOLUTION_SHAPE_CACHE_VERSION,
  snapshotVerificationPolicy,
  withOfflineCheckCacheIdentities,
} from './verifyLockfileCacheIdentities.js'

export interface ReplacedCandidate {
  nonSemverVersion?: string
  registryName?: string
}

export interface VerifyLockfileResolutionsOptions {
  concurrency?: number
  cacheDir?: string
  lockfilePath?: string
  isReplaced?: (name: string, version: string, candidate?: ReplacedCandidate) => boolean
  /** Whether `blockExoticSubdeps` is enforced on lockfile subdependencies. */
  blockExoticSubdeps?: boolean
  /** Trusted workspace importers, deployed workspace dirs, and runtime mirrors for `blockExoticSubdeps`. */
  exoticSubdepsScope?: ExoticSubdepsScope
  localPolicy?: LocalResolutionPolicy
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
 * Designed for fail-closed semantics at the verifier level: a verifier
 * that can't confirm a resolution is expected to return `{ ok: false }`
 * rather than passing silently — otherwise a registry hiccup or an
 * unpublished version would re-open the bypass.
 *
 * Offline dependency-alias checks run before the verification cache lookup,
 * and resolution-shape checks run on every cache miss even when `verifiers`
 * is empty and `blockExoticSubdeps` is off.
 */
export async function verifyLockfileResolutions (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options?: VerifyLockfileResolutionsOptions
): Promise<void> {
  await startLockfileResolutionsVerification(lockfile, verifiers, options)
}

export function verifyLockfileDependencyAliases (lockfile: LockfileObject): void {
  const invalidAliases = collectInvalidAliases(lockfile)
  if (invalidAliases.length > 0) {
    throw buildInvalidAliasError(invalidAliases)
  }
}

export function verifyLockfileFileOffline (lockfile: LockfileFile): void {
  const { invalidAliases, shapeViolations } = collectLockfileFileOfflineViolations(lockfile)
  if (invalidAliases.length > 0) {
    throw buildInvalidAliasError(invalidAliases)
  }
  if (shapeViolations.length > 0) {
    throw buildVerificationError(shapeViolations)
  }
}

export function verifyLockfileResolutionShapes (
  lockfile: LockfileObject,
  localPolicy?: LocalResolutionPolicy,
  lockfilePath?: string
): void {
  verifyLockfileDependencyAliases(lockfile)
  const shapeCtx: FileShapeContext = {
    allowDeployFileUrlKeys: Boolean(localPolicy?.allowDeployFileUrlKeys),
    lockfileDir: localPolicy?.lockfileDir ?? (lockfilePath ? path.dirname(lockfilePath) : undefined),
    skipFileKeyTargetCheck: false,
  }
  const shapeViolations = collectShapeViolations(lockfile, shapeCtx)
  if (shapeViolations.length > 0) {
    throw buildVerificationError(shapeViolations)
  }
}

export function verifyLockfileLocalResolutions (
  lockfile: LockfileObject,
  localPolicy?: LocalResolutionPolicy
): void {
  verifyLockfileDependencyAliases(lockfile)
  const localViolations = collectLocalResolutionViolations(lockfile, localPolicy)
  if (localViolations.length > 0) {
    verifyLockfileResolutionShapes(lockfile, localPolicy)
    throw buildVerificationError(localViolations)
  }
}

/**
 * Runs the synchronous offline prologue of {@link verifyLockfileResolutions}
 * immediately (throwing before any package materialization begins if offline
 * checks fail) and returns a promise only when async {@link ResolutionVerifier}
 * checks remain.
 */
export function startLockfileResolutionsVerification (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options?: VerifyLockfileResolutionsOptions
): Promise<void> | undefined {
  verifyLockfileLocalResolutions(lockfile, options?.localPolicy)
  if (!lockfile.packages) return undefined

  const hasPolicyVerifiers = verifiers.length > 0 || Boolean(options?.blockExoticSubdeps)
  const cacheContext = createVerificationCacheContext(lockfile, verifiers, options, hasPolicyVerifiers)
  if (isVerifiedByCache(cacheContext, hasPolicyVerifiers, options?.lockfilePath)) return

  verifyLockfileResolutionShapes(lockfile, options?.localPolicy, options?.lockfilePath)
  if (!hasPolicyVerifiers) return

  const candidates = collectCandidatesToVerify(lockfile, cacheContext, options)
  if (verifiers.length === 0 || candidates.size === 0) {
    recordVerifiedLockfile(cacheContext)
    return undefined
  }
  return verifyCandidates(candidates, verifiers, { cacheContext, options })
}

function collectCandidatesToVerify (
  lockfile: LockfileObject,
  cacheContext: VerificationCacheContext,
  options: VerifyLockfileResolutionsOptions | undefined
): Map<string, Candidate> {
  if (options?.blockExoticSubdeps) {
    const exoticScan = collectExoticSubdepViolations(lockfile, options.isReplaced, options.exoticSubdepsScope)
    if (exoticScan.violations.length > 0) {
      throw buildVerificationError(exoticScan.violations)
    }
    if (exoticScan.skippedReplaced) {
      cacheContext.cache = undefined
    }
  }
  const candidates = collectCandidates(lockfile)
  if (dropReplacedCandidates(candidates, options?.isReplaced)) {
    cacheContext.cache = undefined
  }
  return candidates
}

type CachePrecomputed = ReturnType<typeof tryLockfileVerificationCache>['precomputed']

interface VerificationCacheContext {
  cache?: { cacheDir: string, lockfilePath?: string }
  cacheVerifiers: VerifierCacheIdentity[]
  hashLockfile: () => string
  precomputed?: CachePrecomputed
}

function createVerificationCacheContext (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options: VerifyLockfileResolutionsOptions | undefined,
  hasPolicyVerifiers: boolean
): VerificationCacheContext {
  let cachedHash: string | undefined
  const useCache = Boolean(options?.cacheDir) && (options?.lockfilePath != null || hasPolicyVerifiers)
  return {
    cache: useCache
      ? { cacheDir: options!.cacheDir!, lockfilePath: options?.lockfilePath }
      : undefined,
    cacheVerifiers: withOfflineCheckCacheIdentities(verifiers, {
      blockExoticSubdeps: options?.blockExoticSubdeps,
      exoticSubdepsScope: options?.exoticSubdepsScope,
    }),
    hashLockfile: () => {
      if (cachedHash == null) cachedHash = hashObject(lockfile)
      return cachedHash
    },
  }
}

function isVerifiedByCache (
  cacheContext: VerificationCacheContext,
  hasPolicyVerifiers: boolean,
  lockfilePath: string | undefined
): boolean {
  if (!cacheContext.cache) return false
  const result = lookupVerificationCache(cacheContext.cache.cacheDir, {
    lockfilePath: cacheContext.cache.lockfilePath,
    verifiers: cacheContext.cacheVerifiers,
    hashLockfile: cacheContext.hashLockfile,
  })
  if (!result.hit) {
    cacheContext.precomputed = result.precomputed
    return false
  }
  // A silent short-circuit looks like the policy gate never ran
  // (pnpm/pnpm#12324), so surface the reused verdict — but only
  // when policy verifiers are active; the shape-only run that
  // every install performs stays quiet.
  if (hasPolicyVerifiers) {
    lockfileVerificationLogger.debug({ status: 'cached', verifiedAt: result.verifiedAt, lockfilePath })
  }
  return true
}

/** Persist the success so the next install can stat-only or content-match the lockfile. */
function recordVerifiedLockfile (cacheContext: VerificationCacheContext): void {
  if (!cacheContext.cache) return
  persistVerificationCache(cacheContext.cache.cacheDir, {
    lockfilePath: cacheContext.cache.lockfilePath,
    verifiers: cacheContext.cacheVerifiers,
    hashLockfile: cacheContext.hashLockfile,
  }, cacheContext.precomputed)
}

function dropReplacedCandidates (
  candidates: Map<string, Candidate>,
  isReplaced: VerifyLockfileResolutionsOptions['isReplaced']
): boolean {
  if (isReplaced == null) return false
  let dropped = false
  for (const [key, { name, version, nonSemverVersion, registryName }] of candidates) {
    if (isReplaced(name, version, { nonSemverVersion, registryName })) {
      candidates.delete(key)
      dropped = true
    }
  }
  return dropped
}

async function verifyCandidates (
  candidates: Map<string, Candidate>,
  verifiers: ResolutionVerifier[],
  { cacheContext, options }: { cacheContext: VerificationCacheContext, options: VerifyLockfileResolutionsOptions | undefined }
): Promise<void> {
  const startedAt = Date.now()
  lockfileVerificationLogger.debug({
    status: 'started',
    entries: candidates.size,
    lockfilePath: options?.lockfilePath,
  })
  let terminalStatus: 'done' | 'failed' = 'failed'
  try {
    const violations = await iterateLockfileViolations(candidates, verifiers, options?.concurrency)
    if (violations.length > 0) throw buildVerificationError(violations)
    terminalStatus = 'done'
    recordVerifiedLockfile(cacheContext)
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
  violations.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  const distinctCodes = new Set(violations.map((violation) => violation.code))
  const isMixed = distinctCodes.size > 1
  const errorCode = isMixed ? 'LOCKFILE_RESOLUTION_VERIFICATION' : violations[0].code
  const visible = violations.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = violations.length - visible.length
  const formatEntry = isMixed
    ? (violation: ResolutionPolicyViolation): string => `  ${violation.name}@${violation.version} [${violation.code}] ${violation.reason}`
    : (violation: ResolutionPolicyViolation): string => `  ${violation.name}@${violation.version} ${violation.reason}`
  const breakdown = visible.map(formatEntry).join('\n')
  const details = omitted > 0 ? `${breakdown}\n  …and ${omitted} more` : breakdown
  return new PnpmError(
    errorCode,
    `${violations.length} lockfile entries failed verification:\n${details}`,
    {
      hint: violations.every((violation) => POLICY_VIOLATION_CODES.has(violation.code))
        ? 'The lockfile contains entries that the active policies reject. ' +
          'This can mean the lockfile is stale, or that someone committed a ' +
          'lockfile that bypassed the policy locally — inspect recent changes ' +
          'to pnpm-lock.yaml before trusting it. If the changes look expected, ' +
          'run "pnpm clean --lockfile" and then "pnpm install" to rebuild from ' +
          'a fresh resolution. If the fresh resolution still fails and you ' +
          'trust the affected packages, relax the policy that flagged them.'
        : 'The lockfile contains entries that pnpm cannot verify, whatever ' +
          'the configured policies. This can mean the lockfile is stale, or ' +
          'that it was tampered with — inspect recent changes to ' +
          'pnpm-lock.yaml before trusting it. If the changes look expected, ' +
          'run "pnpm clean --lockfile" and then "pnpm install" to rebuild ' +
          'from a fresh resolution.',
    }
  )
}

export async function collectResolutionPolicyViolations (
  lockfile: LockfileObject,
  verifiers: ResolutionVerifier[],
  options?: Pick<VerifyLockfileResolutionsOptions, 'concurrency'>
): Promise<ResolutionPolicyViolation[]> {
  if (!lockfile.packages) return []
  const shapeViolations = collectShapeViolations(lockfile)
  if (shapeViolations.length > 0 || verifiers.length === 0) return shapeViolations
  return iterateLockfileViolations(collectCandidates(lockfile), verifiers, options?.concurrency)
}

interface Candidate {
  name: string
  version: string
  nonSemverVersion?: string
  registryName?: string
  resolution: Resolution
}

function collectCandidates (lockfile: LockfileObject): Map<string, Candidate> {
  const candidates = new Map<string, Candidate>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion, registryName } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !version) continue
    const key = `${name}@${version}@${nonSemverVersion ?? ''}@${registryName ?? ''}@${JSON.stringify(snapshot.resolution)}`
    candidates.set(key, {
      name,
      version,
      nonSemverVersion,
      registryName,
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
  let fetchError: unknown
  const limit = pLimit(concurrency ?? DEFAULT_CONCURRENCY)
  const verifyCandidate = async (candidate: Candidate): Promise<void> => {
    try {
      const violation = await findFirstViolation(candidate, verifiers)
      if (violation != null) violations.push(violation)
    } catch (err) {
      fetchError ??= err
    }
  }
  await Promise.all(
    Array.from(candidates.values(), (candidate) => limit(async () => verifyCandidate(candidate)))
  )
  if (fetchError != null) throw fetchError
  return violations
}

async function findFirstViolation (
  { name, version, nonSemverVersion, registryName, resolution }: Candidate,
  verifiers: readonly ResolutionVerifier[]
): Promise<ResolutionPolicyViolation | undefined> {
  for (const verifier of verifiers) {
    // eslint-disable-next-line no-await-in-loop -- the verifiers run in turn so an entry stops at its first failure
    const result = await verifier.verify(resolution, { name, version, nonSemverVersion, registryName })
    if (!result.ok) {
      return { name, version, resolution, code: result.code, reason: result.reason }
    }
  }
  return undefined
}
