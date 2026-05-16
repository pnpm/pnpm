import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { PnpmError } from '@pnpm/error'
import type {
  Resolution,
  ResolutionVerifier,
} from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy, Registries } from '@pnpm/types'
import semver from 'semver'

import type { FetchMetadataFromFromRegistryOptions } from './fetch.js'
import { fetchFullMetadataCached, type FetchFullMetadataCachedOptions } from './fetchFullMetadataCached.js'
import { BUILTIN_NAMED_REGISTRIES } from './parseBareSpecifier.js'

export interface CreateNpmResolutionVerifierOptions {
  /**
   * Minimum age (in minutes) a published version must reach before it is
   * accepted. When unset, the verifier is a no-op for the age check.
   */
  minimumReleaseAge?: number
  /**
   * Gate the age check on strict mode so the built-in default doesn't
   * silently enforce for users who never opted in. The verifier factory
   * returns `undefined` unless both `minimumReleaseAge > 0` and
   * `minimumReleaseAgeStrict` are set.
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
  if (!opts.minimumReleaseAge || !opts.minimumReleaseAgeStrict) return undefined

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

  // In-memory dedup of the time map per (registry, name) for this verifier
  // instance. The on-disk conditional-GET cache is handled inside
  // fetchFullMetadataCached via the resolver's shared mirror at opts.cacheDir.
  const inflight = new Map<string, Promise<Record<string, string | undefined> | undefined>>()
  const fetchTimeMap = async (registry: string, name: string): Promise<Record<string, string | undefined> | undefined> => {
    const cacheKey = `${registry}\x00${name}`
    const cached = inflight.get(cacheKey)
    if (cached) return cached
    const promise = fetchFullMetadataCached(opts.fetchOpts, name, {
      registry,
      authHeaderValue: opts.getAuthHeaderValueByURI(registry),
      cacheDir: opts.cacheDir,
    }).then((meta) => meta.time)
    inflight.set(cacheKey, promise)
    return promise
  }

  return async (resolution, { name, version }) => {
    if (!isNpmRegistryResolution(resolution)) return { ok: true }
    // Non-semver versions identify URL tarballs, file: refs, git refs, etc.
    // The age policy doesn't apply and a registry lookup would 404.
    if (!semver.valid(version)) return { ok: true }
    if (isExcluded(excludePolicy, name, version)) return { ok: true }

    const tarballUrl = (resolution as { tarball?: string }).tarball
    const registry = pickRegistryForVersion(opts.registries, namedRegistryPrefixes, name, tarballUrl)
    let time: Record<string, string | undefined> | undefined
    try {
      time = await fetchTimeMap(registry, name)
    } catch (err) {
      return {
        ok: false,
        code: 'MINIMUM_RELEASE_AGE_VIOLATION',
        reason: uncheckable(err instanceof Error ? err.message : String(err)),
      }
    }
    const published = time?.[version]
    if (!published) {
      // Full metadata is missing this version — either an unpublish or the
      // registry doesn't expose per-version timestamps for it. Either way
      // the release-age can't be verified, so report a violation rather
      // than silently passing.
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
