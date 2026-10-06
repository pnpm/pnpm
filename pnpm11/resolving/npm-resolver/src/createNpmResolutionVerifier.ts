import { createHash } from 'node:crypto'

import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { namedRegistryTarballPrefixes } from '@pnpm/config.pick-registry-for-package'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { isError, PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import {
  isGitHostedTarballUrl,
  type Resolution,
  type ResolutionVerification,
  type ResolutionVerifier,
} from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy, RegistriesByScope, TrustPolicy } from '@pnpm/types'
import semver from 'semver'

import {
  pickRegistryForVersion,
  type RegistryPin,
  type RegistryRouting,
  type ResolutionViolation,
  runRegistryArtifactCheck,
} from './checkRegistryArtifact.js'
import type { FetchMetadataFromFromRegistryOptions } from './fetch.js'
import type { FetchFullMetadataCachedOptions } from './fetchFullMetadataCached.js'
import type { PackageMetaCache } from './pickPackage.js'
import { warnMissingTimeFieldOnce } from './pickPackage.js'
import { failIfTrustDowngraded } from './trustChecks.js'
import {
  createPublishedAtLookupContext,
  fetchFullMetaForTrust,
  fetchFullMetaTime,
  fetchPublishedAt,
  type PublishedAtLookupContext,
} from './verifierMetaLookup.js'
import {
  MINIMUM_RELEASE_AGE_VIOLATION_CODE,
  MISSING_NAMED_REGISTRY_VIOLATION_CODE,
  MISSING_TARBALL_INTEGRITY_VIOLATION_CODE,
  TARBALL_URL_MISMATCH_VIOLATION_CODE,
  TRUST_DOWNGRADE_VIOLATION_CODE,
} from './violationCodes.js'

export interface CreateNpmResolutionVerifierOptions {
  /**
   * Minimum age (in minutes) a published version must reach before it is
   * accepted. When unset, the verifier is a no-op for the age check.
   */
  minimumReleaseAge?: number
  /**
   * Retained on the options bag because the resolver path branches on it
   * (the lowest-version fallback) and tests forward both fields together.
   */
  minimumReleaseAgeStrict?: boolean
  minimumReleaseAgeExclude?: string[]
  /**
   * When the registry's metadata lacks the per-version `time` field
   * (some self-hosted registries strip it), the verifier can't apply
   * the maturity cutoff, and the trust check has no publish order to
   * walk. Set this to `true` to mirror the resolver's warn-and-skip
   * behavior for both — the verifier passes the entry with a one-time
   * `globalWarn`, instead of failing closed. Defaults to `false` so
   * the verifier stays stricter than the resolver only when the user
   * has explicitly opted in to the skip on the resolver side. Scoped
   * to a packument with no usable `time` map: one that dates every
   * version it lists is saying it never published this pin, which
   * fails closed either way.
   */
  ignoreMissingTimeField?: boolean
  /**
   * `'no-downgrade'` rejects a lockfile entry whose version has weaker
   * trust evidence (no attestations) than an earlier-published version
   * had. This mirrors the resolver-time `failIfTrustDowngraded` check
   * applied during fresh resolution — the verifier catches the same
   * supply-chain signal on entries that bypassed resolution (peek-path,
   * frozen lockfile, etc.).
   */
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: string[]
  trustPolicyIgnoreAfter?: number
  registriesByScope: RegistriesByScope
  /**
   * RegistriesByScope reached via the named-registry resolver chain (e.g. `gh:` →
   * GitHub Packages). When a lockfile entry's tarball URL falls under one of
   * these registry base URLs, route the manifest fetch there instead of the
   * scope-derived default.
   */
  registriesByPrefix?: Record<string, string>
  /**
   * Cache-aware full-metadata fetcher. Decoupled from the resolver pipeline
   * so abbreviated metadata and `peekManifestFromStore` fast paths cannot
   * hide the publish timestamp.
   */
  fetchOpts: FetchMetadataFromFromRegistryOptions
  getAuthHeaderValueByURI: GetAuthHeader
  cacheDir?: FetchFullMetadataCachedOptions['cacheDir']
  /**
   * When true, verifier metadata lookups must use the local mirror
   * only and never reach the registry or attestation endpoint.
   */
  offline?: boolean
  /**
   * Per-install LRU shared with the npm resolver's `pickPackage`
   * (`{ get, set }` over `PackageMeta`). When provided, the verifier
   * consults it before fetching: a name the resolver already pulled
   * during the same install yields the cached packument instead of a
   * fresh disk/network round-trip. Optional — frozen-install paths and
   * unit tests don't have a resolver running alongside, in which case
   * the verifier falls back to its own fetch chain.
   */
  metaCache?: PackageMetaCache
  /** Overrides Date.now() for tests. */
  now?: number
}

/**
 * Returns a `ResolutionVerifier` for npm-registry-resolved lockfile
 * entries. It always binds each entry's recorded tarball URL to the
 * artifact the registry's metadata lists (an anti-tamper check that does
 * not depend on any policy), and additionally re-applies the
 * `minimumReleaseAge` and/or `trustPolicy='no-downgrade'` policies when
 * those are configured. Pairs with `createNpmResolver`: each resolver
 * factory may export a sibling verifier factory that the default-resolver
 * combines.
 *
 * Designed for fail-closed semantics: if the manifest can't be loaded or
 * the pinned version is missing from it, the verifier reports a violation
 * rather than silently passing. Mirrors the post-resolution gate bun added
 * for the same shape of bug in oven-sh/bun#30526.
 */
export function createNpmResolutionVerifier (
  opts: CreateNpmResolutionVerifierOptions
): ResolutionVerifier {
  const settings = createVerifierSettings(opts)
  const policy = snapshotVerifierPolicy(opts, settings.mergedRegistriesByPrefix)
  return {
    verify: async (resolution, entry) => verifyResolution(settings, resolution, entry),
    policy,
    canTrustPastCheck: (cached) => canTrustPastCheck(policy, cached),
  }
}

interface VerifierSettings extends RegistryRouting {
  ageCheckActive: boolean
  trustCheckActive: boolean
  cutoff: number
  excludePolicy?: PackageVersionPolicy
  trustExcludePolicy?: PackageVersionPolicy
  mergedRegistriesByPrefix: Record<string, string>
  lookupContext: PublishedAtLookupContext
  trustPolicyIgnoreAfter?: number
  ignoreMissingTimeField: boolean
}

type VerifiedEntry = Parameters<ResolutionVerifier['verify']>[1]

interface PolicyCheckScope {
  ageApplies: boolean
  trustApplies: boolean
}

function createVerifierSettings (opts: CreateNpmResolutionVerifierOptions): VerifierSettings {
  const ageCheckActive = Boolean(opts.minimumReleaseAge)
  const cutoff = ageCheckActive
    ? (opts.now ?? Date.now()) - opts.minimumReleaseAge! * 60 * 1000
    : 0
  const excludePolicy = createOptionalExcludePolicy(opts.minimumReleaseAgeExclude, 'minimumReleaseAgeExclude')
  const trustExcludePolicy = createOptionalExcludePolicy(opts.trustPolicyExclude, 'trustPolicyExclude')
  const mergedRegistriesByPrefix = normalizeRegistriesByPrefix(opts.registriesByPrefix)
  return {
    ageCheckActive,
    trustCheckActive: opts.trustPolicy === 'no-downgrade',
    cutoff,
    excludePolicy,
    trustExcludePolicy,
    mergedRegistriesByPrefix,
    registriesByScope: opts.registriesByScope,
    namedRegistryPrefixes: namedRegistryTarballPrefixes(mergedRegistriesByPrefix),
    lookupContext: createPublishedAtLookupContext(opts, cutoff),
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    ignoreMissingTimeField: opts.ignoreMissingTimeField === true,
  }
}

async function verifyResolution (
  settings: VerifierSettings,
  resolution: Resolution,
  entry: VerifiedEntry
): Promise<ResolutionVerification> {
  if (isVariationsResolution(resolution)) {
    return verifyVariationsResolution(settings, resolution, entry)
  }

  if (!isRegistryTarballResolution(resolution)) return { ok: true }

  // Network-free structural checks must run before registry metadata shortcuts.
  const integrity = (resolution as { integrity?: unknown }).integrity
  if (typeof integrity !== 'string' || integrity.length === 0) {
    return {
      ok: false,
      code: MISSING_TARBALL_INTEGRITY_VIOLATION_CODE,
      reason: 'has no "integrity" field, so its downloaded tarball cannot be verified',
    }
  }

  // URL/git-keyed entries are deliberate non-registry deps. They can still
  // carry a semver `version` copied from the resolved manifest, so the
  // semver guard below isn't enough on its own — the registry policies and
  // the tarball-URL binding don't apply to them, and a registry lookup
  // would 404.
  if (entry.nonSemverVersion != null) return { ok: true }

  const rawTarball = (resolution as { tarball?: unknown }).tarball
  const structuralViolation = findRegistryEntryShapeViolation(entry.version, rawTarball)
  if (structuralViolation) return structuralViolation
  const tarballUrl = typeof rawTarball === 'string' ? rawTarball : undefined
  const registry = pickEntryRegistry(settings, entry, tarballUrl)
  if (typeof registry !== 'string') return registry

  return verifyAgainstRegistry(settings, {
    registry,
    name: entry.name,
    version: entry.version,
    integrity,
    rawRevision: (resolution as { revision?: unknown }).revision,
    tarballUrl,
  })
}

function isVariationsResolution (resolution: Resolution): resolution is Resolution & { type: 'variations', variants?: Array<{ resolution?: Resolution }> } {
  return resolution != null && typeof resolution === 'object' && (resolution as { type?: unknown }).type === 'variations'
}

async function verifyVariationsResolution (
  settings: VerifierSettings,
  resolution: { variants?: Array<{ resolution?: Resolution }> },
  entry: VerifiedEntry
): Promise<ResolutionVerification> {
  for (const variant of resolution.variants ?? []) {
    if (variant?.resolution) {
      // eslint-disable-next-line no-await-in-loop -- sequential variant verification stops at first violation
      const result = await verifyResolution(settings, variant.resolution, entry)
      if (!result.ok) return result
    }
  }
  return { ok: true }
}

function findRegistryEntryShapeViolation (version: string, rawTarball: unknown): ResolutionViolation | undefined {
  if (!semver.valid(version)) {
    return {
      ok: false,
      code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
      reason: `has a non-semver version ("${version}") and so cannot be verified against the registry's published metadata`,
    }
  }
  if (rawTarball != null && typeof rawTarball !== 'string') {
    return {
      ok: false,
      code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
      reason: 'has a non-string "tarball" field, so its URL cannot be verified',
    }
  }
  return undefined
}

function pickEntryRegistry (
  settings: VerifierSettings,
  entry: VerifiedEntry,
  tarballUrl: string | undefined
): string | ResolutionViolation {
  if (entry.registryName == null) return pickRegistryForVersion(settings, entry.name, tarballUrl)
  // Registry-qualified entries name their registry in the dep path, so
  // routing does not depend on a recorded tarball URL (canonical URLs
  // are omitted from the lockfile in the 12.0 format).
  const namedRegistry = settings.mergedRegistriesByPrefix[entry.registryName]
  if (!namedRegistry) {
    // Fail closed: without the registry URL, none of the metadata-backed
    // checks below can vouch for this entry.
    return {
      ok: false,
      code: MISSING_NAMED_REGISTRY_VIOLATION_CODE,
      reason: `has registry prefix '${entry.registryName}:', which is not declared by the registries setting`,
    }
  }
  return namedRegistry
}

async function verifyAgainstRegistry (
  settings: VerifierSettings,
  pin: RegistryPin
): Promise<ResolutionVerification> {
  const scope: PolicyCheckScope = {
    ageApplies: settings.ageCheckActive && !isExcluded(settings.excludePolicy, pin.name, pin.version),
    trustApplies: settings.trustCheckActive && !isExcluded(settings.trustExcludePolicy, pin.name, pin.version),
  }
  // A registry entry that pins an explicit tarball URL must point at the
  // artifact the registry's own metadata lists. Otherwise a trusted
  // `name@version` could front bytes from an attacker-chosen URL (with a
  // matching integrity for those bytes). This binding is unconditional —
  // it does not depend on `minimumReleaseAge`/`trustPolicy` and isn't
  // narrowed by their exclude lists, since it guards integrity rather
  // than maturity/trust. Registry entries with no tarball URL reconstruct
  // it from name+version+registry, so they're inherently bound.
  const policyApplies = scope.ageApplies || scope.trustApplies
  if (pin.tarballUrl != null || (pin.rawRevision != null && policyApplies)) {
    const artifactViolation = await runRegistryArtifactCheck(settings.lookupContext, pin)
    if (artifactViolation) return artifactViolation
  }
  return (await runPolicyChecks(settings, pin, scope)) ?? { ok: true }
}

async function runPolicyChecks (
  settings: VerifierSettings,
  pin: RegistryPin,
  scope: PolicyCheckScope
): Promise<ResolutionViolation | undefined> {
  if (scope.ageApplies) {
    const ageViolation = await runAgeCheck(settings.lookupContext, pin, settings)
    if (ageViolation) return ageViolation
  }
  if (!scope.trustApplies) return undefined
  return runTrustCheck(settings.lookupContext, pin, {
    trustPolicyExclude: settings.trustExcludePolicy,
    trustPolicyIgnoreAfter: settings.trustPolicyIgnoreAfter,
    ignoreMissingTimeField: settings.ignoreMissingTimeField,
  })
}

// Snapshot the exclude lists (sorted, deduped) and require an exact
// match in `canTrustPastCheck`: cache identity == policy identity.
// Any change to either exclude list — adding, removing, or
// substituting an entry — invalidates the cached run. This is
// stricter than a pure correctness check would require (adding to
// either list is more permissive and the cached pass would still
// hold), but it makes the cache contract trivial to reason about and
// removes a class of bypasses where a previously-approved version
// stays trusted after its exclude entry has been pulled.
type VerifierPolicy = {
  tarballUrlBinding: true
  revisionHistoryBinding: true
  integrityRequired: true
  variationResolutionsVerified: true
  namedRegistriesRouting: string
  minimumReleaseAge: number
  minimumReleaseAgeExclude: string[]
  trustPolicy: TrustPolicy | null
  trustPolicyExclude: string[]
  trustPolicyIgnoreAfter: number | null
  minimumReleaseAgeIgnoreMissingTime: boolean
}

function snapshotVerifierPolicy (
  opts: CreateNpmResolutionVerifierOptions,
  mergedRegistriesByPrefix: Record<string, string>
): VerifierPolicy {
  const sortedRegistriesByPrefix = Object.fromEntries(
    Object.entries(mergedRegistriesByPrefix).sort(([aliasA], [aliasB]) => aliasA.localeCompare(aliasB))
  )
  return {
    // Marks runs that enforced the tarball-URL binding.
    tarballUrlBinding: true,
    revisionHistoryBinding: true,
    // Same cache identity rule for the missing-integrity structural check.
    integrityRequired: true,
    variationResolutionsVerified: true,
    namedRegistriesRouting: createHash('sha256')
      .update(JSON.stringify(sortedRegistriesByPrefix))
      .digest('hex'),
    minimumReleaseAge: opts.minimumReleaseAge ?? 0,
    minimumReleaseAgeExclude: sortUniquePatterns(opts.minimumReleaseAgeExclude),
    trustPolicy: opts.trustPolicy ?? null,
    trustPolicyExclude: sortUniquePatterns(opts.trustPolicyExclude),
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter ?? null,
    minimumReleaseAgeIgnoreMissingTime: opts.ignoreMissingTimeField === true,
  }
}

function sortUniquePatterns (patterns: string[] | undefined): string[] {
  return [...new Set(patterns ?? [])].sort()
}

function canTrustPastCheck (current: VerifierPolicy, cached: Record<string, unknown>): boolean {
  return cachedRunEnforcedUnconditionalChecks(cached, current.namedRegistriesRouting) &&
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

function cachedRunEnforcedUnconditionalChecks (cached: Record<string, unknown>, namedRegistriesRouting: string): boolean {
  // The tarball-URL binding is unconditional today; a cached run that
  // didn't record it can't be trusted to have enforced it.
  if (cached.tarballUrlBinding !== true) return false
  if (cached.revisionHistoryBinding !== true) return false

  // The missing-integrity check is also unconditional; older cache records
  // without the flag cannot prove they rejected unverifiable tarballs.
  if (cached.integrityRequired !== true) return false
  if (cached.variationResolutionsVerified !== true) return false

  return cached.namedRegistriesRouting === namedRegistriesRouting
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

async function runAgeCheck (
  context: PublishedAtLookupContext,
  pin: RegistryPin,
  opts: { cutoff: number, ignoreMissingTimeField: boolean }
): Promise<ResolutionViolation | undefined> {
  // A transport failure (auth/network/5xx) propagates the registry's own fetch
  // error (e.g. ERR_PNPM_FETCH_403); the gate aborts the install with it rather
  // than folding it into a policy violation. A successful fetch that simply
  // lacks a publish timestamp for this version is handled below.
  const published = await fetchPublishedAt(context, pin.registry, pin.name, pin.version)
  if (!published) return reportMissingPublishTime(context, pin, opts.ignoreMissingTimeField)
  const publishedAt = new Date(published)
  const publishedMs = publishedAt.getTime()
  if (Number.isNaN(publishedMs)) {
    return {
      ok: false,
      code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
      reason: 'publish timestamp is not a valid date',
    }
  }
  if (publishedMs > opts.cutoff) {
    return {
      ok: false,
      code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
      reason: `was published at ${publishedAt.toISOString()}, within the minimumReleaseAge cutoff (${new Date(opts.cutoff).toISOString()})`,
    }
  }
  return undefined
}

/**
 * No source — attestation, local mirror, or full metadata — surfaced a
 * publish timestamp for this version. What
 * `minimumReleaseAgeIgnoreMissingTime` opts out of is a registry that
 * cannot date its releases, so the skip is granted only when the
 * packument carries no usable `time` map at all — the same shape the
 * resolver's `pickMatchingVersionFinal` warns and skips on, so the
 * verifier can't be stricter than fresh resolution. A packument that
 * does date every version it lists is instead telling us this pin is
 * not one of them (`dropIncompletePublishTimes` leaves no partial maps
 * for that to be ambiguous), and an unpublished or never-published pin
 * must fail closed however the flag is set.
 */
async function reportMissingPublishTime (
  context: PublishedAtLookupContext,
  pin: RegistryPin,
  ignoreMissingTimeField: boolean
): Promise<ResolutionViolation | undefined> {
  if (ignoreMissingTimeField) {
    // Already awaited by the lookup above, so this is a cache hit.
    const timeMap = await fetchFullMetaTime(context, pin.registry, pin.name)
    if (timeMap == null) {
      warnMissingTimeFieldOnce(pin.name, 'minimumReleaseAge')
      return undefined
    }
  }
  return {
    ok: false,
    code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
    reason: uncheckable('minimumReleaseAge', 'version not present in registry manifest'),
  }
}

/**
 * Run the resolver-time `failIfTrustDowngraded` check against the
 * pinned lockfile version. The packument is fetched through a
 * per-install cache so multiple versions of the same package share
 * one fetch.
 *
 * No attestation fast-path here even though the per-version
 * attestation endpoint is cheaper than the packument: presence of
 * provenance on the current version is not sufficient to clear a
 * downgrade. A package could have shipped earlier versions under a
 * `trustedPublisher` with provenance (the higher-rank evidence) and
 * then dropped back to plain provenance for the version we're verifying —
 * `failIfTrustDowngraded` correctly flags that, and a "has any
 * attestation → pass" shortcut would silently miss it.
 */
async function runTrustCheck (
  context: PublishedAtLookupContext,
  pin: RegistryPin,
  opts: {
    trustPolicyExclude?: PackageVersionPolicy
    trustPolicyIgnoreAfter?: number
    ignoreMissingTimeField?: boolean
  }
): Promise<ResolutionViolation | undefined> {
  const meta = await fetchFullMetaForTrust(context, pin.registry, pin.name)

  try {
    failIfTrustDowngraded(meta, pin.version, opts)
  } catch (err) {
    return {
      ok: false,
      code: TRUST_DOWNGRADE_VIOLATION_CODE,
      reason: isError(err) ? err.message : String(err),
    }
  }
  return undefined
}

function tryParseUrl (url: string): URL | null {
  try {
    return new URL(url)
  } catch {
    return null
  }
}

function uncheckable (policy: 'minimumReleaseAge' | 'trustPolicy', why: string): string {
  return `could not be checked against ${policy} (${why})`
}

function createOptionalExcludePolicy (patterns: string[] | undefined, key: string): PackageVersionPolicy | undefined {
  return patterns?.length ? createExcludePolicy(patterns, key) : undefined
}

function createExcludePolicy (patterns: string[], key: string): PackageVersionPolicy {
  // Mirror the wrapping done by the full-resolution path
  // (installing/deps-resolver/src/resolveDependencyTree.ts) so the error
  // code is identical regardless of which path surfaced the invalid pattern.
  try {
    return createPackageVersionPolicy(patterns)
  } catch (err) {
    if (!err || typeof err !== 'object' || !('message' in err)) throw err
    throw new PnpmError(
      `INVALID_${key.replace(/([A-Z])/g, '_$1').toUpperCase()}`,
      `Invalid value in ${key}: ${(err as { message: string }).message}`
    )
  }
}

function isExcluded (policy: PackageVersionPolicy | undefined, name: string, version: string): boolean {
  if (!policy) return false
  const result = policy(name)
  if (result === true) return true
  if (Array.isArray(result) && result.includes(version)) return true
  return false
}

function isRegistryTarballResolution (resolution: Resolution | unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  // Only plain tarball resolutions (npm registry / named registries) have no
  // `type` field. Git / directory / binary / custom resolutions all carry one.
  if ('type' in resolution && (resolution as { type?: unknown }).type != null) return false
  const tarball = (resolution as { tarball?: unknown }).tarball
  if (typeof tarball === 'string') {
    // Git-hosted tarballs (codeload/gitlab/bitbucket) are special-cased in
    // the resolver and aren't subject to registry policy.
    if (isGitHostedTarballUrl(tarball)) return false
    // Local/non-registry tarballs (for example `file:`) have no packument
    // metadata, so minimumReleaseAge/trustPolicy verification cannot apply.
    const protocol = tryParseUrl(tarball)?.protocol
    if (protocol != null && protocol !== 'http:' && protocol !== 'https:') return false
  }
  // Canonical registry entries may omit both `tarball` and `integrity`.
  return true
}
