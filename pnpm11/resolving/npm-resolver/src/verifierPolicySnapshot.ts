import { createHash } from 'node:crypto'

import { normalizeRegistryKey } from '@pnpm/config.pick-registry-for-package'
import type { RegistriesByScope, TrustPolicy } from '@pnpm/types'

import type { CreateNpmResolutionVerifierOptions } from './createNpmResolutionVerifier.js'

// Snapshot the exclude lists (sorted, deduped) and require an exact
// match in `canTrustPastCheck`: cache identity == policy identity.
// Any change to either exclude list — adding, removing, or
// substituting an entry — invalidates the cached run. This is
// stricter than a pure correctness check would require (adding to
// either list is more permissive and the cached pass would still
// hold), but it makes the cache contract trivial to reason about and
// removes a class of bypasses where a previously-approved version
// stays trusted after its exclude entry has been pulled.
export interface VerifierPolicy {
  [key: string]: unknown
  tarballUrlBinding: true
  integrityRequired: true
  variationResolutionsVerified: true
  namedRegistriesRouting: string
  registriesRouting: string
  minimumReleaseAge: number
  minimumReleaseAgeExclude: string[]
  trustPolicy: TrustPolicy | null
  trustPolicyExclude: string[]
  trustPolicyIgnoreAfter: number | null
  minimumReleaseAgeIgnoreMissingTime: boolean
}

export function snapshotVerifierPolicy (
  opts: CreateNpmResolutionVerifierOptions,
  mergedRegistriesByPrefix: Record<string, string>
): VerifierPolicy {
  const sortedRegistriesByPrefix = Object.fromEntries(
    Object.entries(mergedRegistriesByPrefix).sort(([aliasA], [aliasB]) => aliasA.localeCompare(aliasB))
  )
  return {
    // Marks runs that enforced the tarball-URL binding.
    tarballUrlBinding: true,
    // Same cache identity rule for the missing-integrity structural check.
    integrityRequired: true,
    variationResolutionsVerified: true,
    namedRegistriesRouting: createHash('sha256')
      .update(JSON.stringify(sortedRegistriesByPrefix))
      .digest('hex'),
    registriesRouting: hashRegistriesRouting(opts.registriesByScope),
    minimumReleaseAge: opts.minimumReleaseAge ?? 0,
    minimumReleaseAgeExclude: sortUniquePatterns(opts.minimumReleaseAgeExclude),
    trustPolicy: opts.trustPolicy ?? null,
    trustPolicyExclude: sortUniquePatterns(opts.trustPolicyExclude),
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter ?? null,
    minimumReleaseAgeIgnoreMissingTime: opts.ignoreMissingTimeField === true,
  }
}

export function canTrustPastCheck (current: VerifierPolicy, cached: Record<string, unknown>): boolean {
  return cachedRunEnforcedUnconditionalChecks(current, cached) &&
    cachedMinimumReleaseAgeCovers(current, cached) &&
    cachedTrustPolicyMatches(current, cached) &&
    // Missing-time tolerance: a cached run that failed closed on an
    // absent `time` field accepted a subset of what today's tolerant
    // policy accepts, so it stays trustworthy. Turning the tolerance
    // off invalidates it — entries the past run waved through are the
    // ones today's policy exists to reject. Older records (no field)
    // read as intolerant, which is the safe direction.
    !(cached.minimumReleaseAgeIgnoreMissingTime === true && !current.minimumReleaseAgeIgnoreMissingTime)
}

function hashRegistriesRouting (registriesByScope: RegistriesByScope): string {
  const sortedEntries = Object.entries(registriesByScope)
    .filter((entry): entry is [string, string] => typeof entry[1] === 'string')
    .sort(([scopeA], [scopeB]) => (scopeA < scopeB ? -1 : scopeA > scopeB ? 1 : 0))
    .map(([scope, registry]) => [scope, canonicalRegistryUrl(registry)])
  return createHash('sha256')
    .update(JSON.stringify(Object.fromEntries(sortedEntries)))
    .digest('hex')
}

function canonicalRegistryUrl (registry: string): string {
  const normalized = normalizeRegistryKey(registry)
  if (normalized != null && !normalized.startsWith('null/')) return normalized
  return registry.endsWith('/') ? registry : `${registry}/`
}

function sortUniquePatterns (patterns: string[] | undefined): string[] {
  return [...new Set(patterns ?? [])].sort()
}

function cachedRunEnforcedUnconditionalChecks (current: VerifierPolicy, cached: Record<string, unknown>): boolean {
  // The tarball-URL binding is unconditional today; a cached run that
  // didn't record it can't be trusted to have enforced it.
  if (cached.tarballUrlBinding !== true) return false

  // The missing-integrity check is also unconditional; older cache records
  // without the flag cannot prove they rejected unverifiable tarballs.
  if (cached.integrityRequired !== true) return false
  if (cached.variationResolutionsVerified !== true) return false

  return cached.namedRegistriesRouting === current.namedRegistriesRouting &&
    cached.registriesRouting === current.registriesRouting
}

function cachedMinimumReleaseAgeCovers (current: VerifierPolicy, cached: Record<string, unknown>): boolean {
  // Maturity: a previously cached run under a larger cutoff
  // (stricter window) is trustworthy under a smaller current one —
  // its set of accepted versions is a subset of today's. The
  // reverse — tightening the cutoff — invalidates the cached run:
  // versions that passed before may now be in-window. Non-number
  // cached values come from an older record shape and aren't trusted.
  const past = cached.minimumReleaseAge
  const pastNumber = typeof past === 'number' ? past : 0
  if (pastNumber < current.minimumReleaseAge) return false

  // Excludes: today's sorted-deduped lists must match the cached
  // ones byte for byte. Older records (no field) fall back to an
  // empty array, so they only trust today's empty policy.
  return sameExcludeList(cached.minimumReleaseAgeExclude, current.minimumReleaseAgeExclude)
}

// Trust policy: any change to `trustPolicy`, the exclude list, or
// the ignore-after cutoff invalidates the cached run. Older
// records (no trust field at all) treat the trust policy as
// absent and are only trusted under an unset-today policy.
function cachedTrustPolicyMatches (current: VerifierPolicy, cached: Record<string, unknown>): boolean {
  if ((cached.trustPolicy ?? null) !== current.trustPolicy) return false
  if (!sameExcludeList(cached.trustPolicyExclude, current.trustPolicyExclude)) return false
  const pastIgnoreAfter = typeof cached.trustPolicyIgnoreAfter === 'number'
    ? cached.trustPolicyIgnoreAfter
    : null
  return pastIgnoreAfter === current.trustPolicyIgnoreAfter
}

function sameExcludeList (cachedExcludes: unknown, currentExcludes: string[]): boolean {
  const pastExcludes = Array.isArray(cachedExcludes) ? cachedExcludes : []
  return JSON.stringify(pastExcludes) === JSON.stringify(currentExcludes)
}
