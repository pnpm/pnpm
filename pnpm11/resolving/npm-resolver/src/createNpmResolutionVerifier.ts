import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import {
  isGitHostedTarballUrl,
  type Resolution,
  type ResolutionVerifier,
} from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy, Registries, TrustPolicy } from '@pnpm/types'
import semver from 'semver'

import type { FetchMetadataFromFromRegistryOptions } from './fetch.js'
import { fetchAttestationPublishedAt } from './fetchAttestationPublishedAt.js'
import {
  fetchAbbreviatedMetadataCached,
  fetchFullMetadataCached,
  type FetchFullMetadataCachedOptions,
} from './fetchFullMetadataCached.js'
import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'
import { BUILTIN_NAMED_REGISTRIES } from './parseBareSpecifier.js'
import type { PackageMetaCache } from './pickPackage.js'
import { getPkgMetaCacheKey, getPkgMirrorPath, loadMeta, warnMissingTimeFieldOnce } from './pickPackage.js'
import { failIfTrustDowngraded } from './trustChecks.js'
import {
  MINIMUM_RELEASE_AGE_VIOLATION_CODE,
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
   * The verifier itself no longer gates on this flag — once the loose-mode
   * auto-collect makes every accepted-immature pin explicit in
   * `minimumReleaseAgeExclude`, running the verifier in loose mode is the
   * thing that proves the manifest stays in sync with the lockfile.
   */
  minimumReleaseAgeStrict?: boolean
  minimumReleaseAgeExclude?: string[]
  /**
   * When the registry's metadata lacks the per-version `time` field
   * (some self-hosted registries strip it), the verifier can't apply
   * the maturity cutoff. Set this to `true` to mirror the resolver's
   * `pickMatchingVersionFinal` warn-and-skip behavior — the verifier
   * passes the entry with a one-time `globalWarn`, instead of failing
   * closed. Defaults to `false` so the verifier stays stricter than
   * the resolver only when the user has explicitly opted in to the
   * skip on the resolver side.
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
  registries: Registries
  /**
   * Registries reached via the named-registry resolver chain (e.g. `gh:` →
   * GitHub Packages). When a lockfile entry's tarball URL falls under one of
   * these registry base URLs, route the manifest fetch there instead of the
   * scope-derived default.
   */
  namedRegistries?: Record<string, string>
  /**
   * Cache-aware full-metadata fetcher. Decoupled from the resolver pipeline
   * so abbreviated metadata and `peekManifestFromStore` fast paths cannot
   * hide the publish timestamp.
   */
  fetchOpts: FetchMetadataFromFromRegistryOptions
  getAuthHeaderValueByURI: GetAuthHeader
  cacheDir?: FetchFullMetadataCachedOptions['cacheDir']
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
  const ageCheckActive = Boolean(opts.minimumReleaseAge)
  const trustCheckActive = opts.trustPolicy === 'no-downgrade'

  const cutoff = ageCheckActive
    ? (opts.now ?? Date.now()) - opts.minimumReleaseAge! * 60 * 1000
    : 0
  const excludePolicy = opts.minimumReleaseAgeExclude?.length
    ? createExcludePolicy(opts.minimumReleaseAgeExclude, 'minimumReleaseAgeExclude')
    : undefined
  const trustExcludePolicy = opts.trustPolicyExclude?.length
    ? createExcludePolicy(opts.trustPolicyExclude, 'trustPolicyExclude')
    : undefined

  // Pre-normalize named-registry URLs and sort by length so two registries
  // that share a hostname but differ by path (e.g. `https://npm/team-a/` vs
  // `https://npm/team-b/`) route to the longest matching prefix — matching
  // only `origin` would silently send lookups to the wrong one. Built-in
  // aliases (`gh:` → npm.pkg.github.com, etc.) are merged in alongside the
  // user-defined ones so the verifier recognizes the same set of named
  // registries the resolver does; otherwise a package resolved via `gh:`
  // would land in the lockfile with a tarball URL the verifier can't route.
  const namedRegistryPrefixes = Object.values({
    ...BUILTIN_NAMED_REGISTRIES,
    ...(opts.namedRegistries ?? {}),
  })
    .map((url) => {
      const parsed = tryParseUrl(url)
      if (!parsed) return null
      // Ensure trailing slash so prefix matching against tarball URLs (which
      // always include the package path under the registry root) does not
      // accidentally match a sibling registry whose URL shares a prefix string.
      const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
      return `${parsed.origin}${pathname}`
    })
    .filter((value): value is string => value != null)
    .sort((a, b) => b.length - a.length)

  // Per-install dedup of every network/disk fetch the verifier issues.
  // The maturity check uses the layered `fetchPublishedAt` lookup; the
  // trust check uses an attestation fast-path before falling back to
  // the same full-metadata mirror. All maps live here so verifying
  // many versions of the same package only pays the disk/network costs
  // once. The on-disk conditional-GET cache is handled inside
  // fetch{Abbreviated,Full}MetadataCached via the resolver's shared
  // mirrors at opts.cacheDir.
  const lookupContext: PublishedAtLookupContext = {
    fetchOpts: opts.fetchOpts,
    getAuthHeaderValueByURI: opts.getAuthHeaderValueByURI,
    cacheDir: opts.cacheDir,
    cutoffMs: cutoff,
    sharedMetaCache: opts.metaCache,
    abbreviatedMetaCache: new Map(),
    publishedAtCache: new Map(),
    localMetaCache: new Map(),
    fullMetaCache: new Map(),
    fullMetaForTrustCache: new Map(),
  }

  const minimumReleaseAge = opts.minimumReleaseAge ?? 0
  const trustPolicy = opts.trustPolicy
  const trustPolicyIgnoreAfter = opts.trustPolicyIgnoreAfter

  const verify: ResolutionVerifier['verify'] = async (resolution, { name, version, nonSemverVersion }) => {
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
    if (nonSemverVersion != null) return { ok: true }

    if (!semver.valid(version)) {
      return {
        ok: false,
        code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
        reason: `has a non-semver version ("${version}") and so cannot be verified against the registry's published metadata`,
      }
    }

    const rawTarball = (resolution as { tarball?: unknown }).tarball
    if (rawTarball != null && typeof rawTarball !== 'string') {
      return {
        ok: false,
        code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
        reason: 'has a non-string "tarball" field, so its URL cannot be verified',
      }
    }
    const tarballUrl = typeof rawTarball === 'string' ? rawTarball : undefined
    const registry = pickRegistryForVersion(opts.registries, namedRegistryPrefixes, name, tarballUrl)

    // A registry entry that pins an explicit tarball URL must point at the
    // artifact the registry's own metadata lists. Otherwise a trusted
    // `name@version` could front bytes from an attacker-chosen URL (with a
    // matching integrity for those bytes). This binding is unconditional —
    // it does not depend on `minimumReleaseAge`/`trustPolicy` and isn't
    // narrowed by their exclude lists, since it guards integrity rather
    // than maturity/trust. Registry entries with no tarball URL reconstruct
    // it from name+version+registry, so they're inherently bound.
    if (typeof tarballUrl === 'string') {
      const urlViolation = await runTarballUrlCheck(lookupContext, registry, name, version, tarballUrl)
      if (urlViolation) return urlViolation
    }

    const ageApplies = ageCheckActive && !isExcluded(excludePolicy, name, version)
    const trustApplies = trustCheckActive && !isExcluded(trustExcludePolicy, name, version)
    if (!ageApplies && !trustApplies) return { ok: true }

    if (ageApplies) {
      const ageViolation = await runAgeCheck(lookupContext, registry, name, version, cutoff, opts.ignoreMissingTimeField === true)
      if (ageViolation) return ageViolation
    }

    if (trustApplies) {
      const trustViolation = await runTrustCheck(lookupContext, registry, name, version, {
        trustPolicyExclude: trustExcludePolicy,
        trustPolicyIgnoreAfter,
      })
      if (trustViolation) return trustViolation
    }

    return { ok: true }
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
  const sortedMinAgeExcludes = [...new Set(opts.minimumReleaseAgeExclude ?? [])].sort()
  const sortedTrustExcludes = [...new Set(opts.trustPolicyExclude ?? [])].sort()
  return {
    verify,
    policy: {
      // Marks runs that enforced the tarball-URL binding. A cache record
      // written before this rule existed lacks the flag, so
      // `canTrustPastCheck` rejects it and forces a re-verification that
      // applies the binding — otherwise an upgrade could keep trusting a
      // lockfile that was only ever age/trust-checked.
      tarballUrlBinding: true,
      // Same cache identity rule for the missing-integrity structural check.
      integrityRequired: true,
      minimumReleaseAge,
      minimumReleaseAgeExclude: sortedMinAgeExcludes,
      trustPolicy: trustPolicy ?? null,
      trustPolicyExclude: sortedTrustExcludes,
      trustPolicyIgnoreAfter: trustPolicyIgnoreAfter ?? null,
    },
    canTrustPastCheck: (cached) => {
      // The tarball-URL binding is unconditional today; a cached run that
      // didn't record it can't be trusted to have enforced it.
      if (cached.tarballUrlBinding !== true) return false

      // The missing-integrity check is also unconditional; older cache records
      // without the flag cannot prove they rejected unverifiable tarballs.
      if (cached.integrityRequired !== true) return false

      // Maturity: a previously cached run under a larger cutoff
      // (stricter window) is trustworthy under a smaller current one —
      // its set of accepted versions is a subset of today's. The
      // reverse — tightening the cutoff — invalidates the cached run:
      // versions that passed before may now be in-window. Non-number
      // cached values come from an older record shape and aren't trusted.
      const past = cached.minimumReleaseAge
      const pastNumber = typeof past === 'number' ? past : 0
      if (pastNumber < minimumReleaseAge) return false

      // Excludes: today's sorted-deduped lists must match the cached
      // ones byte for byte. Older records (no field) fall back to an
      // empty array, so they only trust today's empty policy.
      const pastMinAgeExcludes = Array.isArray(cached.minimumReleaseAgeExclude)
        ? cached.minimumReleaseAgeExclude
        : []
      if (JSON.stringify(pastMinAgeExcludes) !== JSON.stringify(sortedMinAgeExcludes)) return false

      // Trust policy: any change to `trustPolicy`, the exclude list, or
      // the ignore-after cutoff invalidates the cached run. Older
      // records (no trust field at all) treat the trust policy as
      // absent and are only trusted under an unset-today policy.
      const pastTrustPolicy = cached.trustPolicy ?? null
      const todayTrustPolicy = trustPolicy ?? null
      if (pastTrustPolicy !== todayTrustPolicy) return false
      const pastTrustExcludes = Array.isArray(cached.trustPolicyExclude)
        ? cached.trustPolicyExclude
        : []
      if (JSON.stringify(pastTrustExcludes) !== JSON.stringify(sortedTrustExcludes)) return false
      const pastIgnoreAfter = typeof cached.trustPolicyIgnoreAfter === 'number'
        ? cached.trustPolicyIgnoreAfter
        : null
      const todayIgnoreAfter = trustPolicyIgnoreAfter ?? null
      if (pastIgnoreAfter !== todayIgnoreAfter) return false

      return true
    },
  }
}

async function runAgeCheck (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string,
  cutoff: number,
  ignoreMissingTimeField: boolean
): Promise<{ ok: false, code: string, reason: string } | undefined> {
  // A transport failure (auth/network/5xx) propagates the registry's own fetch
  // error (e.g. ERR_PNPM_FETCH_403); the gate aborts the install with it rather
  // than folding it into a policy violation. A successful fetch that simply
  // lacks a publish timestamp for this version is handled below.
  const published = await fetchPublishedAt(context, registry, name, version)
  if (!published) {
    // No source — attestation, local mirror, or full metadata —
    // surfaced a publish timestamp for this version. The resolver's
    // pickMatchingVersionFinal honors `minimumReleaseAgeIgnoreMissingTime`
    // for the same shape (some self-hosted registries strip per-version
    // `time`); the verifier mirrors that so it can't be stricter than
    // fresh resolution. Without the flag we still fail closed — better
    // a false reject than silent bypass when the user hasn't opted in.
    if (ignoreMissingTimeField) {
      warnMissingTimeFieldOnce(name)
      return undefined
    }
    return {
      ok: false,
      code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
      reason: uncheckable('minimumReleaseAge', 'version not present in registry manifest'),
    }
  }
  const publishedAt = new Date(published)
  const ts = publishedAt.getTime()
  if (Number.isNaN(ts)) {
    return {
      ok: false,
      code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
      reason: 'publish timestamp is not a valid date',
    }
  }
  if (ts > cutoff) {
    return {
      ok: false,
      code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
      reason: `was published at ${publishedAt.toISOString()}, within the minimumReleaseAge cutoff (${new Date(cutoff).toISOString()})`,
    }
  }
  return undefined
}

/**
 * Confirm the lockfile-pinned tarball URL is the artifact the registry's
 * own metadata lists for this exact `name@version`.
 *
 * Fail-closed: the entry passes only when the registry metadata
 * affirmatively lists this version with a matching tarball URL. If the
 * metadata can't be fetched, doesn't list the version, or omits
 * `dist.tarball`, the entry can't be confirmed and is rejected — otherwise
 * a tampered lockfile could smuggle a malicious URL past the check by
 * pointing it at a `name@version` the registry can't vouch for.
 */
async function runTarballUrlCheck (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string,
  lockfileTarball: string
): Promise<{ ok: false, code: string, reason: string } | undefined> {
  const { meta, error } = await fetchAbbreviatedMeta(context, registry, name)
  if (error != null) {
    // Couldn't reach the registry to verify (auth/network/5xx). Propagate the
    // registry's own fetch error (e.g. ERR_PNPM_FETCH_403, which already
    // explains the auth situation) instead of mislabeling a transport failure
    // as a tampering-style URL mismatch. The gate aborts the install with that
    // error — still fail-closed, the entry never reaches the filesystem.
    throw error
  }
  const registryTarball = meta?.versionTarballs?.get(version)
  if (registryTarball != null && sameTarballUrl(lockfileTarball, registryTarball)) {
    return undefined
  }
  return {
    ok: false,
    code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
    reason: registryTarball == null
      ? "could not be verified against the registry's published metadata"
      : `has a tarball URL (${lockfileTarball}) that does not match the registry's published metadata (${registryTarball})`,
  }
}

function sameTarballUrl (a: string, b: string): boolean {
  return canonicalTarballUrl(a) === canonicalTarballUrl(b)
}

// Mirror the tolerance toLockfileResolution applies when it decides whether
// a tarball URL is "the expected one": ignore the protocol and `%2f` scope
// encoding so a benign http/https or encoding difference isn't read as
// tampering. The `%2f` match is case-insensitive because `normalizeRegistryUrl`
// (`new URL().toString()`) can upper-case percent-escapes to `%2F`.
function canonicalTarballUrl (url: string): string {
  const normalized = normalizeRegistryUrl(url).replace(/%2f/gi, '/')
  const schemeEnd = normalized.indexOf('://')
  return schemeEnd === -1 ? normalized : normalized.slice(schemeEnd + 3)
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
  registry: string,
  name: string,
  version: string,
  opts: {
    trustPolicyExclude?: PackageVersionPolicy
    trustPolicyIgnoreAfter?: number
  }
): Promise<{ ok: false, code: string, reason: string } | undefined> {
  // A transport failure (auth/network/5xx) propagates the registry's own fetch
  // error; the gate aborts the install with it rather than folding it into a
  // policy violation. Still fail-closed: a missing manifest can't be mistaken
  // for a passing trust check because the install never proceeds.
  const meta = await fetchFullMetaForTrust(context, registry, name)

  try {
    failIfTrustDowngraded(meta, version, opts)
  } catch (err) {
    return {
      ok: false,
      code: TRUST_DOWNGRADE_VIOLATION_CODE,
      reason: err instanceof Error ? err.message : String(err),
    }
  }
  return undefined
}

function fetchFullMetaForTrust (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<PackageMeta> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.fullMetaForTrustCache.get(cacheKey)
  if (cachedPromise == null) {
    // Fast path: if the resolver already upgraded to full meta for this
    // (registry, name) during the same install (e.g. minimumReleaseAge
    // active), reuse that document. Abbreviated meta is rejected here —
    // it lacks per-version `time` and per-version trust evidence, both
    // required by failIfTrustDowngraded. The read is registry-qualified
    // (see `getPkgMetaCacheKey`), so a package of the same name served by
    // a different registry can't be returned here.
    const shared = readSharedMetaForTrust(context.sharedMetaCache, registry, name)
    if (shared != null) {
      cachedPromise = Promise.resolve(projectTrustMeta(shared))
    } else {
      // Don't swallow the fetch rejection here — `runTrustCheck` catches it
      // and surfaces the underlying message in the violation reason, which
      // is more actionable than the generic "metadata is unavailable" the
      // `!meta` fallback emits. The cache still holds the rejected promise
      // so repeat verifier calls for the same (registry, name) within one
      // install don't refetch a known-failing endpoint.
      //
      // The fetched packument is projected down to just the trust-relevant
      // fields (per-version `_npmUser.trustedPublisher` and
      // `dist.attestations.provenance`, plus the package-level `time` map)
      // before being stored. The full document — dependency maps, scripts,
      // READMEs for every version — would otherwise stay resident in this
      // map for the entire install, which on multi-thousand-entry
      // workspaces OOMs CI runners with a 2GB heap (see #11860).
      cachedPromise = fetchFullMetadataCached(context.fetchOpts, name, {
        registry,
        authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
        cacheDir: context.cacheDir,
      }).then(projectTrustMeta)
    }
    context.fullMetaForTrustCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

// Project the full packument to a minimal `PackageMeta`-shaped view
// that exposes only the fields `failIfTrustDowngraded` reads:
//   • `name` and `modified` for error messages and cache keys
//   • `time` for the per-version publish-date walk
//   • `versions[v]._npmUser.trustedPublisher`
//   • `versions[v].dist.attestations.provenance`
// The shape is still a valid `PackageMeta` so the downstream consumer
// doesn't have to special-case it — only the bulk fields (dependency
// graph, scripts, README, etc.) are dropped.
function projectTrustMeta (meta: PackageMeta): PackageMeta {
  const versions: Record<string, PackageInRegistry> = {}
  for (const [version, manifest] of Object.entries(meta.versions ?? {})) {
    versions[version] = projectTrustManifest(manifest)
  }
  return {
    name: meta.name,
    'dist-tags': {},
    versions,
    time: meta.time,
    modified: meta.modified,
    etag: meta.etag,
  }
}

function projectTrustManifest (manifest: PackageInRegistry): PackageInRegistry {
  // Drop everything except the trust-evidence fields. `PackageInRegistry.dist`
  // is typed as requiring `shasum` and `tarball`, but the trust check never
  // reads them; cast away the unsoundness so callers see the same nominal
  // shape without the per-version dependency graph / scripts / README bulk
  // carrying through. `_npmUser` is similarly narrowed to just
  // `trustedPublisher` and `approver` — the only sub-fields the trust check
  // inspects — so we don't keep maintainer name/email PII resident in the
  // cache.
  const approver = manifest._npmUser?.approver
  const trustedPublisher = manifest._npmUser?.trustedPublisher
  const provenance = manifest.dist?.attestations?.provenance
  let npmUser: PackageInRegistry['_npmUser'] = undefined
  if (approver) {
    npmUser ||= {}
    npmUser.approver = {}
  }
  if (trustedPublisher) {
    npmUser ||= {}
    npmUser.trustedPublisher = trustedPublisher
  }
  return {
    _npmUser: npmUser,
    dist: provenance != null
      ? { attestations: { provenance } }
      : undefined,
  } as unknown as PackageInRegistry
}

type PublishedAtTimeMap = Record<string, string | undefined>

interface PublishedAtLookupContext {
  fetchOpts: FetchMetadataFromFromRegistryOptions
  getAuthHeaderValueByURI: GetAuthHeader
  cacheDir?: string
  /**
   * The `minimumReleaseAge` cutoff converted to a unix-ms epoch. A
   * version with a publish time strictly less than this passes the
   * policy. Used by the abbreviated-metadata shortcut: if the
   * package's last-modified time is older than the cutoff, every
   * version it contains is too.
   */
  cutoffMs: number
  /**
   * Resolver-owned LRU (per-install) keyed via `getPkgMetaCacheKey`
   * (registry + name, with a `:full` suffix for full meta). When the
   * resolver has already fetched a package during this install, the
   * verifier reuses that packument instead of re-paying the disk/network
   * round-trip — the fresh-install path otherwise fetches every entry
   * twice. Optional:
   * the frozen-install path runs without a resolver and never
   * populates this cache, so the verifier's own fetch chain still
   * carries the cold case.
   */
  sharedMetaCache?: PackageMetaCache
  /**
   * Per-(registry+name) memo of the abbreviated metadata fetch.
   * Abbreviated is what the resolver populates by default, so on a
   * non-frozen install the conditional GET hits the disk mirror at
   * ~zero cost. Stores only the two fields the shortcut reads —
   * package-level `modified` plus the set of currently-listed version
   * names — so the multi-hundred-KB packument can be GC'd as soon as
   * the fetch returns (the cache only needs to dedupe network/disk
   * round-trips, not full document storage). Resolves to `{ meta }` on
   * success or `{ error }` on a fetch failure — it never rejects, so the
   * cached promise is safe to share between callers.
   */
  abbreviatedMetaCache: Map<string, Promise<AbbreviatedMetaResult>>
  /**
   * Per-(registry+name+version) memo of the final published-at answer
   * the verifier hands to the policy check. One install verifies each
   * (name, version) pair at most once.
   */
  publishedAtCache: Map<string, Promise<string | undefined>>
  /**
   * Per-(registry+name) memo of the on-disk full-metadata mirror read.
   * One disk read per package regardless of how many versions we
   * verify of it.
   */
  localMetaCache: Map<string, Promise<PublishedAtTimeMap | undefined>>
  /**
   * Per-(registry+name) memo of the full-metadata network fetch — only
   * issued when both the abbreviated-modified shortcut and the
   * attestation endpoint fail to yield a timestamp.
   */
  fullMetaCache: Map<string, Promise<PublishedAtTimeMap | undefined>>
  /**
   * Per-(registry+name) memo of the full packument used by the trust
   * check (history walk for `failIfTrustDowngraded`). Kept separate
   * from `fullMetaCache` because the trust check needs the whole
   * document (`_npmUser`, `dist.attestations` per version) where the
   * age check only needs `time`.
   */
  fullMetaForTrustCache: Map<string, Promise<PackageMeta>>
}

/**
 * Per-(registry, name, version) lookup with a layered fallback:
 *
 * 1. **Abbreviated metadata `modified` shortcut.** This is what the
 *    resolver already fetches by default; it's a small document with
 *    a package-level last-modified time but no per-version timestamps.
 *    If `modified` is older than the policy cutoff, every version in
 *    this package was published at least that long ago — return the
 *    `modified` timestamp as a conservative upper bound and skip the
 *    rest of the chain. Costs one conditional GET that the resolver
 *    has usually already paid for.
 * 2. **On-disk full-metadata mirror.** If a previous verification
 *    populated `FULL_META_DIR`, take the per-version timestamp from
 *    there.
 * 3. **npm attestation endpoint.** Small payload, just this version's
 *    Sigstore-anchored timestamp. Wins on cold cache when the package
 *    was published with provenance.
 * 4. **Full metadata fetch.** Last resort — only paid when the
 *    abbreviated shortcut can't decide, the local full mirror is
 *    cold, and there's no attestation.
 */
async function fetchPublishedAt (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string
): Promise<string | undefined> {
  const cacheKey = `${registry}\x00${name}\x00${version}`
  let cachedPromise = context.publishedAtCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = resolvePublishedAt(context, registry, name, version)
    context.publishedAtCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

async function resolvePublishedAt (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string
): Promise<string | undefined> {
  const abbreviatedShortcut = await tryAbbreviatedModifiedShortcut(context, registry, name, version)
  if (abbreviatedShortcut != null) return abbreviatedShortcut

  const localTime = await readLocalMetaTime(context, registry, name)
  if (localTime?.[version]) return localTime[version]

  const attestationTime = await fetchAttestationPublishedAt(context.fetchOpts, name, version, {
    registry,
    authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
  })
  if (attestationTime != null) return attestationTime

  const fullMetaTime = await fetchFullMetaTime(context, registry, name)
  return fullMetaTime?.[version]
}

/**
 * Returns the abbreviated metadata's `modified` timestamp **iff** it
 * proves the gate would pass — i.e. modified is strictly older than
 * the policy cutoff *and* the pinned version still exists in the
 * package's current versions map.
 *
 * The version check is the fail-closed contract: an unpublished or
 * never-published version must not slip through on the package-level
 * `modified` timestamp. When the version is missing here we fall
 * through to the later layers so the caller eventually surfaces the
 * "version not present in registry manifest" violation.
 *
 * Returns `undefined` otherwise (modified is too recent, the metadata
 * lacks a parseable modified field, the version isn't in the abbreviated
 * form, or the fetch failed) and the caller proceeds with per-version
 * lookups.
 */
async function tryAbbreviatedModifiedShortcut (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string
): Promise<string | undefined> {
  // A fetch failure here is fine: ignore `error` and fall back to per-version
  // lookups, the same as a successful-but-uninformative metadata response.
  const { meta } = await fetchAbbreviatedMeta(context, registry, name)
  const modified = meta?.modified
  if (typeof modified !== 'string') return undefined
  const modifiedMs = Date.parse(modified)
  if (Number.isNaN(modifiedMs)) return undefined
  if (modifiedMs >= context.cutoffMs) return undefined
  // The shortcut treats `modified` as an upper bound on every version's
  // publish time — but only for versions the registry currently lists.
  // An unpublished or never-published pin would otherwise pass the gate
  // on a stale package-level timestamp.
  if (!meta?.versionTarballs?.has(version)) return undefined
  return modified
}

function fetchAbbreviatedMeta (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<AbbreviatedMetaResult> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.abbreviatedMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    // Fast path: the resolver's per-install LRU already holds this
    // packument from its own pickPackage pass — abbreviated or full.
    // Project it for the shortcut and skip the disk/network round-trip.
    // The read is registry-qualified (see `getPkgMetaCacheKey`), so it
    // can only return this registry's own packument.
    const shared = readSharedMeta(context.sharedMetaCache, registry, name)
    if (shared != null) {
      cachedPromise = Promise.resolve({ meta: projectAbbreviatedMeta(shared) })
    } else {
      // Carry a fetch failure (auth/network/5xx) as `error` instead of
      // collapsing it to `undefined`: the tarball-URL check rethrows it (so the
      // registry's own error surfaces, not a tampering-style mismatch) while
      // the age shortcut ignores it and falls back to per-version lookups.
      // Keeping it a resolved value — not a rejected promise — lets the two
      // callers share one cached promise without an unhandled rejection.
      cachedPromise = fetchAbbreviatedMetadataCached(context.fetchOpts, name, {
        registry,
        authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
        cacheDir: context.cacheDir,
      }).then(
        (meta) => ({ meta: projectAbbreviatedMeta(meta) }),
        (error: unknown) => ({ error })
      )
    }
    context.abbreviatedMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

function readSharedMeta (
  cache: PackageMetaCache | undefined,
  registry: string,
  name: string
): PackageMeta | undefined {
  if (cache == null) return undefined
  // Prefer a full entry — it carries every field the abbreviated form
  // does, plus `time` and per-version trust evidence the trust check
  // needs. The resolver only populates a full key when the install ran
  // with `minimumReleaseAge` configured, otherwise the bare key holds
  // the abbreviated form.
  return readSharedFullMeta(cache, registry, name) ??
    validateSharedMeta(cache.get(getPkgMetaCacheKey(registry, name, false, false)), name)
}

function readSharedMetaForTrust (
  cache: PackageMetaCache | undefined,
  registry: string,
  name: string
): PackageMeta | undefined {
  if (cache == null) return undefined
  // Abbreviated meta is rejected for the trust check — it lacks
  // per-version `time` and per-version trust evidence.
  return readSharedFullMeta(cache, registry, name)
}

// The resolver keys full metadata as either filtered or unfiltered
// depending on its own `filterMetadata` setting; the verifier doesn't
// know which, and a filtered full packument keeps everything the
// verifier reads (`time`, per-version `_npmUser`, `dist`), so try both.
function readSharedFullMeta (
  cache: PackageMetaCache,
  registry: string,
  name: string
): PackageMeta | undefined {
  return validateSharedMeta(cache.get(getPkgMetaCacheKey(registry, name, true, false)), name) ??
    validateSharedMeta(cache.get(getPkgMetaCacheKey(registry, name, true, true)), name)
}

// Defensive guard against the resolver's `metaCache` returning an
// unexpected entry. The cache key is registry-qualified (see
// `getPkgMetaCacheKey`), so a package of the same name from another
// registry can't be returned; this name check catches accidental
// returns of a different package (cache corruption, factory misuse)
// rather than silently feeding wrong data to the trust / age check.
function validateSharedMeta (meta: PackageMeta | undefined, name: string): PackageMeta | undefined {
  if (meta == null) return undefined
  if (meta.name !== name) return undefined
  return meta
}

// Project the abbreviated packument down to the few fields the verifier
// actually reads — package-level `modified`, plus a per-version map of
// `dist.tarball` (whose keys double as the version-existence set for the
// `tryAbbreviatedModifiedShortcut` check and the tarball-URL binding). The
// resolver populates the abbreviated mirror with every version's
// dependency / engine / dist info, which can run to hundreds of KB per
// package and accumulate to many GB across a multi-thousand-entry
// lockfile (see #11860). The full document is GC-able as soon as this
// closure returns; only the short tarball-URL strings are retained.
function projectAbbreviatedMeta (meta: PackageMeta): AbbreviatedMetaProjection {
  let versionTarballs: Map<string, string | undefined> | undefined
  if (meta.versions) {
    versionTarballs = new Map()
    for (const [version, manifest] of Object.entries(meta.versions)) {
      versionTarballs.set(version, manifest.dist?.tarball)
    }
  }
  return {
    modified: meta.modified,
    versionTarballs,
  }
}

interface AbbreviatedMetaProjection {
  modified?: string
  /** version → `dist.tarball`; key presence means the version is published. */
  versionTarballs?: Map<string, string | undefined>
}

/**
 * Result of an abbreviated-metadata fetch. The fetch error is carried as a
 * value rather than rejected so the per-install cache can hold a single
 * resolved promise — a cached rejection would surface as an unhandled
 * rejection and be shared across every caller of the same key. The
 * tarball-URL check rethrows this error; the age shortcut ignores it and
 * falls back to per-version lookups.
 *
 * Modeled as a discriminated union so a result carries exactly one of `meta`
 * or `error` — never both, never neither.
 */
type AbbreviatedMetaResult =
  | { meta: AbbreviatedMetaProjection, error?: undefined }
  | { error: unknown, meta?: undefined }

function readLocalMetaTime (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<PublishedAtTimeMap | undefined> {
  if (!context.cacheDir) return Promise.resolve(undefined)
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.localMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = loadLocalMetaTime(context.cacheDir, registry, name)
    context.localMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

async function loadLocalMetaTime (
  cacheDir: string,
  registry: string,
  name: string
): Promise<PublishedAtTimeMap | undefined> {
  const pkgMirror = getPkgMirrorPath(cacheDir, FULL_META_DIR, registry, name)
  const cached = await loadMeta(pkgMirror)
  return cached?.time
}

function fetchFullMetaTime (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<PublishedAtTimeMap | undefined> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.fullMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = fetchFullMetadataCached(context.fetchOpts, name, {
      registry,
      authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
      cacheDir: context.cacheDir,
    }).then((meta) => meta.time)
    context.fullMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

function pickRegistryForVersion (
  registries: Registries,
  namedRegistryPrefixes: string[],
  name: string,
  tarballUrl: string | undefined
): string {
  // If the lockfile records where the tarball lives, prefer that — scope
  // routing (`@scope:registry`) only covers scoped packages, but named
  // registries (`gh:`, `jsr:` aliases, custom) ship un-scoped packages whose
  // origin we'd otherwise miss. Match the longest prefix so that two named
  // registries sharing a host but differing by path don't collide.
  if (tarballUrl) {
    // Match on the same canonical form the tarball comparison uses, so a
    // named-registry tarball that differs from the configured base only by
    // scheme or `%2f` encoding still routes to its registry instead of
    // falling back (and then failing closed against the wrong packument).
    const normalized = canonicalTarballUrl(tarballUrl)
    for (const prefix of namedRegistryPrefixes) {
      if (normalized.startsWith(canonicalTarballUrl(prefix))) return prefix
    }
  }
  return pickRegistryForPackage(registries, name)
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
