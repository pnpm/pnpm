import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { FULL_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type {
  Resolution,
  ResolutionVerifier,
} from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy, Registries } from '@pnpm/types'
import semver from 'semver'

import type { FetchMetadataFromFromRegistryOptions } from './fetch.js'
import { fetchAttestationPublishedAt } from './fetchAttestationPublishedAt.js'
import {
  fetchAbbreviatedMetadataCached,
  fetchFullMetadataCached,
  type FetchFullMetadataCachedOptions,
} from './fetchFullMetadataCached.js'
import { BUILTIN_NAMED_REGISTRIES } from './parseBareSpecifier.js'
import { getPkgMirrorPath, loadMeta } from './pickPackage.js'

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
  getAuthHeaderValueByURI: (registry: string) => string | undefined
  cacheDir?: FetchFullMetadataCachedOptions['cacheDir']
  /** Overrides Date.now() for tests. */
  now?: number
}

/**
 * Returns a `ResolutionVerifier` that re-applies the `minimumReleaseAge`
 * policy to npm-registry-resolved lockfile entries, or `undefined` when no
 * policy is active. Pairs with `createNpmResolver`: each resolver factory
 * may export a sibling verifier factory that the default-resolver combines.
 *
 * Designed for fail-closed semantics: if the manifest can't be loaded or
 * the pinned version is missing from it, the verifier reports a violation
 * rather than silently passing. Mirrors the post-resolution gate bun added
 * for the same shape of bug in oven-sh/bun#30526.
 */
export function createNpmResolutionVerifier (
  opts: CreateNpmResolutionVerifierOptions
): ResolutionVerifier | undefined {
  if (!opts.minimumReleaseAge) return undefined

  const cutoff = (opts.now ?? Date.now()) - opts.minimumReleaseAge * 60 * 1000
  const excludePolicy = opts.minimumReleaseAgeExclude?.length
    ? createExcludePolicy(opts.minimumReleaseAgeExclude)
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

  // Per-install dedup of every network/disk fetch the verifier issues
  // (see fetchPublishedAt for the lookup order). The on-disk
  // conditional-GET cache is handled inside fetch{Abbreviated,Full}MetadataCached
  // via the resolver's shared mirrors at opts.cacheDir.
  const lookupContext: PublishedAtLookupContext = {
    fetchOpts: opts.fetchOpts,
    getAuthHeaderValueByURI: opts.getAuthHeaderValueByURI,
    cacheDir: opts.cacheDir,
    cutoffMs: cutoff,
    abbreviatedMetaCache: new Map(),
    publishedAtCache: new Map(),
    localMetaCache: new Map(),
    fullMetaCache: new Map(),
  }

  const minimumReleaseAge = opts.minimumReleaseAge

  const verify: ResolutionVerifier['verify'] = async (resolution, { name, version }) => {
    if (!isNpmRegistryResolution(resolution)) return { ok: true }
    // Non-semver versions identify URL tarballs, file: refs, git refs, etc.
    // The age policy doesn't apply and a registry lookup would 404.
    if (!semver.valid(version)) return { ok: true }
    if (isExcluded(excludePolicy, name, version)) return { ok: true }

    const tarballUrl = (resolution as { tarball?: string }).tarball
    const registry = pickRegistryForVersion(opts.registries, namedRegistryPrefixes, name, tarballUrl)
    let published: string | undefined
    try {
      published = await fetchPublishedAt(lookupContext, registry, name, version)
    } catch (err) {
      return {
        ok: false,
        code: 'MINIMUM_RELEASE_AGE_VIOLATION',
        reason: uncheckable(err instanceof Error ? err.message : String(err)),
      }
    }
    if (!published) {
      // No source — attestation, local mirror, or full metadata —
      // surfaced a publish timestamp for this version. Either it's
      // unpublished or the registry doesn't expose per-version
      // timestamps. Report a violation rather than silently passing.
      return {
        ok: false,
        code: 'MINIMUM_RELEASE_AGE_VIOLATION',
        reason: uncheckable('version not present in registry manifest'),
      }
    }
    const publishedAt = new Date(published)
    const ts = publishedAt.getTime()
    if (Number.isNaN(ts)) {
      return {
        ok: false,
        code: 'MINIMUM_RELEASE_AGE_VIOLATION',
        reason: 'publish timestamp is not a valid date',
      }
    }
    if (ts > cutoff) {
      return {
        ok: false,
        code: 'MINIMUM_RELEASE_AGE_VIOLATION',
        reason: `was published at ${publishedAt.toISOString()}, within the minimumReleaseAge cutoff (${new Date(cutoff).toISOString()})`,
      }
    }
    return { ok: true }
  }
  return {
    verify,
    policy: { minimumReleaseAge },
    canTrustPastCheck: (cached) => {
      // A previously cached run under a larger cutoff (stricter window)
      // is trustworthy under a smaller current one — its set of
      // accepted versions is a subset of today's. The reverse —
      // tightening the cutoff — invalidates the cached run: versions
      // that passed before may now be in-window. Non-number cached
      // values come from an older record shape and aren't trusted.
      const past = cached.minimumReleaseAge
      return typeof past === 'number' && past >= minimumReleaseAge
    },
  }
}

type PublishedAtTimeMap = Record<string, string | undefined>

interface PublishedAtLookupContext {
  fetchOpts: FetchMetadataFromFromRegistryOptions
  getAuthHeaderValueByURI: (registry: string) => string | undefined
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
   * Per-(registry+name) memo of the abbreviated metadata fetch.
   * Abbreviated is what the resolver populates by default, so on a
   * non-frozen install the conditional GET hits the disk mirror at
   * ~zero cost. Resolves to the parsed metadata or `undefined` on
   * failure.
   */
  abbreviatedMetaCache: Map<string, Promise<{ modified?: string } | undefined>>
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
  const abbreviatedShortcut = await tryAbbreviatedModifiedShortcut(context, registry, name)
  if (abbreviatedShortcut != null) return abbreviatedShortcut

  const localTime = await readLocalMetaTime(context, registry, name)
  if (localTime?.[version]) return localTime[version]

  const attestationTime = await fetchAttestationPublishedAt(context.fetchOpts, name, version, {
    registry,
    authHeaderValue: context.getAuthHeaderValueByURI(registry),
  })
  if (attestationTime != null) return attestationTime

  const fullMetaTime = await fetchFullMetaTime(context, registry, name)
  return fullMetaTime?.[version]
}

/**
 * Returns the abbreviated metadata's `modified` timestamp **iff** it
 * proves the gate would pass — i.e. modified is strictly older than
 * the policy cutoff. In that case every version this package contains
 * predates the cutoff, so the caller can short-circuit with `modified`
 * as a conservative upper-bound publish time.
 *
 * Returns `undefined` otherwise (modified is too recent, the metadata
 * lacks a parseable modified field, or the fetch failed) and the
 * caller proceeds with per-version lookups.
 */
async function tryAbbreviatedModifiedShortcut (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<string | undefined> {
  const meta = await fetchAbbreviatedMeta(context, registry, name)
  const modified = meta?.modified
  if (typeof modified !== 'string') return undefined
  const modifiedMs = Date.parse(modified)
  if (Number.isNaN(modifiedMs)) return undefined
  if (modifiedMs >= context.cutoffMs) return undefined
  return modified
}

function fetchAbbreviatedMeta (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<{ modified?: string } | undefined> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.abbreviatedMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = fetchAbbreviatedMetadataCached(context.fetchOpts, name, {
      registry,
      authHeaderValue: context.getAuthHeaderValueByURI(registry),
      cacheDir: context.cacheDir,
    }).catch(() => undefined)
    context.abbreviatedMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

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
      authHeaderValue: context.getAuthHeaderValueByURI(registry),
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
    const normalized = tryParseUrl(tarballUrl)?.toString()
    if (normalized) {
      for (const prefix of namedRegistryPrefixes) {
        if (normalized.startsWith(prefix)) return prefix
      }
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

function uncheckable (why: string): string {
  return `could not be checked against minimumReleaseAge (${why})`
}

function createExcludePolicy (patterns: string[]): PackageVersionPolicy {
  // Mirror the wrapping done by the full-resolution path
  // (installing/deps-resolver/src/resolveDependencyTree.ts) so the error
  // code is identical regardless of which path surfaced the invalid pattern.
  try {
    return createPackageVersionPolicy(patterns)
  } catch (err) {
    if (!err || typeof err !== 'object' || !('message' in err)) throw err
    throw new PnpmError(
      'INVALID_MINIMUM_RELEASE_AGE_EXCLUDE',
      `Invalid value in minimumReleaseAgeExclude: ${(err as { message: string }).message}`
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

function isNpmRegistryResolution (resolution: Resolution | unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  // Only plain tarball resolutions (npm registry / named registries) have no
  // `type` field. Git / directory / binary / custom resolutions all carry one.
  if ('type' in resolution && (resolution as { type?: unknown }).type != null) return false
  // Git-hosted tarballs (codeload/gitlab/bitbucket) are special-cased in
  // the resolver and aren't subject to release-age policy.
  if ('gitHosted' in resolution && (resolution as { gitHosted?: boolean }).gitHosted) return false
  return 'tarball' in resolution || 'integrity' in resolution
}
