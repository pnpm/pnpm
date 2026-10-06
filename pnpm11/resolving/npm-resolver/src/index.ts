import type {
  FetchFromRegistry,
  GetAuthHeader,
  RetryTimeoutOptions,
} from '@pnpm/fetching.types'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { storeIndexKey } from '@pnpm/store.index'
import type {
  DependencyManifest,
  RegistriesByScope,
} from '@pnpm/types'
import {
  readPkgFromCafs,
} from '@pnpm/worker'
import { LRUCache } from 'lru-cache'

import { clearMeta, retainsFullMeta } from './clearMeta.js'
import { fetchMetadataFromFromRegistry, type FetchMetadataFromFromRegistryOptions, RegistryResponseError } from './fetch.js'
import { memoizeFetchMetadata } from './memoizeFetchMetadata.js'
import { mergeNamedRegistries } from './namedRegistries.js'
import {
  BUILTIN_REGISTRIES_BY_PREFIX,
  type NpmAliasTarget,
  parseBareSpecifier,
  parseNpmAliasTarget,
  type RegistryPackageSpec,
} from './parseBareSpecifier.js'
import {
  type PackageMetaCache,
  pickPackage,
} from './pickPackage.js'
import { pickPackageFromMeta, pickVersionByVersionRange } from './pickPackageFromMeta.js'
import { resolveFromNamedRegistry, resolveJsr } from './resolveFromPrefixedRegistry.js'
import { createResolveLatest, isJsrSpec, isNamedRegistrySpec, isNpmSpec } from './resolveLatest.js'
import { resolveNpm } from './resolveNpm.js'
import type {
  NpmResolver,
  ResolveFromNpmContext,
  ResolveLatestFromNpmStyle,
} from './resolverTypes.js'
import { workspacePrefToNpm } from './workspacePrefToNpm.js'

export {
  BUILTIN_REGISTRIES_BY_PREFIX,
  fetchMetadataFromFromRegistry,
  type FetchMetadataFromFromRegistryOptions,
  type NpmAliasTarget,
  type PackageMeta,
  type PackageMetaCache,
  parseBareSpecifier,
  parseNpmAliasTarget,
  pickPackageFromMeta,
  pickVersionByVersionRange,
  type RegistryPackageSpec,
  RegistryResponseError,
  workspacePrefToNpm,
}
export { createNpmResolutionVerifier, type CreateNpmResolutionVerifierOptions } from './createNpmResolutionVerifier.js'
export { decodeRegistry, encodeRegistry } from './encodeRegistry.js'
export { formatTimeAgo } from './formatTimeAgo.js'
export { loadMeta, type LoadMetaOptions } from './metaMirror.js'
export { NoMatchingVersionError, type NoMatchingVersionErrorOptions } from './NoMatchingVersionError.js'
export { detectMinReleaseAgeViolation } from './releaseAgePolicy.js'
export type {
  JsrResolveResult,
  NamedRegistryResolveResult,
  NpmResolver,
  NpmResolveResult,
  ResolveFromNpmContext,
  ResolveFromNpmOptions,
  ResolveLatestFromNpmStyle,
  WorkspaceResolveResult,
} from './resolverTypes.js'
export {
  MINIMUM_RELEASE_AGE_VIOLATION_CODE,
  TRUST_DOWNGRADE_VIOLATION_CODE,
} from './violationCodes.js'
export { pickMatchingLocalVersionOrNull } from './workspaceResolution.js'

export interface ResolverFactoryOptions {
  cacheDir: string
  storeDir?: string
  frozenStore?: boolean
  fullMetadata?: boolean
  /**
   * Asked instead of {@link ResolverFactoryOptions.fullMetadata} when the
   * caller can answer per registry — a registry that declares
   * `supportsTimeField` needs no full metadata for a time-based resolution
   * even when the others do.
   */
  needsFullMetadataFor?: (registry: string) => boolean
  filterMetadata?: boolean
  offline?: boolean
  preferOffline?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  saveWorkspaceProtocol?: boolean | 'rolling'
  preserveAbsolutePaths?: boolean
  ignoreMissingTimeField?: boolean
  fetchWarnTimeoutMs?: number
  /** Pre-populated metadata cache. When provided, the resolver uses this
   *  instead of creating a new LRU cache. Useful for servers that keep
   *  metadata in SQLite or persist it across requests. */
  metaCache?: PackageMetaCache
}

type MemoizedFetchMetadata = ReturnType<typeof memoizeFetchMetadata>

export function createNpmResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: ResolverFactoryOptions
): {
  resolveFromNpm: NpmResolver
  resolveFromJsr: NpmResolver
  resolveFromNamedRegistry: NpmResolver
  resolveLatestFromNpm: ResolveLatestFromNpmStyle
  resolveLatestFromJsr: ResolveLatestFromNpmStyle
  resolveLatestFromNamedRegistry: ResolveLatestFromNpmStyle
  clearCache: () => void
} {
  if (typeof opts.cacheDir !== 'string') {
    throw new TypeError('`opts.cacheDir` is required and needs to be a string')
  }
  const fetchOpts: FetchMetadataFromFromRegistryOptions = {
    fetch: fetchFromRegistry,
    retry: opts.retry ?? {},
    timeout: opts.timeout ?? 60000,
    fetchWarnTimeoutMs: opts.fetchWarnTimeoutMs ?? 10 * 1000, // 10 sec
  }
  const memoizedFetch = memoizeFetchMetadata(fetchMetadataFromFromRegistry.bind(null, fetchOpts), {
    condenseSettledMeta: retainsFullMeta(opts) ? undefined : clearMeta,
  })
  // Track ownership so `clearCache()` below only wipes the in-memory
  // cache when this factory created it. A caller-supplied
  // `opts.metaCache` may be shared with another resolver instance (or
  // outlive this resolver entirely — e.g. a long-lived agent process
  // that keeps one cache across many install requests); clearing it
  // here would silently evict entries that other consumers are still
  // using.
  const ownsMetaCache = opts.metaCache == null
  const metaCache: PackageMetaCache = opts.metaCache ?? createDefaultPackageMetaCache()
  const ctx = createResolveFromNpmContext({ fetch: memoizedFetch.fetch, metaCache, getAuthHeader, opts })
  const boundResolveFromNpm = resolveNpm.bind(null, ctx)
  const boundResolveFromJsr = resolveJsr.bind(null, ctx)
  const boundResolveFromNamedRegistry = resolveFromNamedRegistry.bind(null, ctx)
  const defaultRegistry = opts.registriesByScope.default
  return {
    resolveFromNpm: boundResolveFromNpm,
    resolveFromJsr: boundResolveFromJsr,
    resolveFromNamedRegistry: boundResolveFromNamedRegistry,
    resolveLatestFromNpm: createResolveLatest(boundResolveFromNpm,
      (query) => isNpmSpec(query, defaultRegistry)),
    resolveLatestFromJsr: createResolveLatest(boundResolveFromJsr, isJsrSpec),
    resolveLatestFromNamedRegistry: createResolveLatest(boundResolveFromNamedRegistry,
      (query) => isNamedRegistrySpec(query, ctx.namedRegistryNames)),
    clearCache: () => {
      if (ownsMetaCache && 'clear' in metaCache && typeof metaCache.clear === 'function') {
        metaCache.clear()
      }
      memoizedFetch.clear()
    },
  }
}

function createResolveFromNpmContext ({ fetch, metaCache, getAuthHeader, opts }: {
  fetch: MemoizedFetchMetadata['fetch']
  metaCache: PackageMetaCache
  getAuthHeader: GetAuthHeader
  opts: ResolverFactoryOptions
}): ResolveFromNpmContext {
  // This marker is intentionally resolver-scoped rather than attached to
  // `metaCache`: callers may reuse one metadata cache across installs, and a
  // later install must retry a full-metadata upgrade that previously got 304.
  const releaseAgeUpgradeCheckedPackuments = new WeakSet<PackageMeta>()
  const peekManifestFromStore = opts.storeDir
    ? createStoreManifestPeeker(opts.storeDir, opts.frozenStore)
    : undefined
  const registriesByPrefix = mergeNamedRegistries(opts.registriesByPrefix)
  return {
    getAuthHeaderValueByURI: getAuthHeader,
    pickPackage: pickPackage.bind(null, {
      fetch,
      fullMetadata: opts.fullMetadata,
      needsFullMetadataFor: opts.needsFullMetadataFor,
      filterMetadata: opts.filterMetadata,
      metaCache,
      offline: opts.offline,
      preferOffline: opts.preferOffline,
      cacheDir: opts.cacheDir,
      ignoreMissingTimeField: opts.ignoreMissingTimeField,
      releaseAgeUpgradeCheckedPackuments,
      peekManifestFromStore,
    }),
    registriesByScope: opts.registriesByScope,
    registriesByPrefix,
    namedRegistryNames: new Set(Object.keys(registriesByPrefix)),
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    ignoreMissingTimeField: opts.ignoreMissingTimeField,
    peekManifestFromStore,
    warnedHeldBackUpdates: new Set(),
    warnedTrustDowngradeFallbacks: new Set(),
  }
}

function createStoreManifestPeeker (
  storeDir: string,
  frozenStore: boolean | undefined
): NonNullable<ResolveFromNpmContext['peekManifestFromStore']> {
  const peekLockerForPeek = new Map<string, Promise<DependencyManifest | undefined>>()
  return async (peekOpts) => {
    const filesIndexFile = storeIndexKey(peekOpts.integrity, peekOpts.id)
    const existingRequest = peekLockerForPeek.get(filesIndexFile)
    if (existingRequest != null) {
      return existingRequest
    }
    const request = readPkgFromCafs(
      {
        storeDir,
        verifyStoreIntegrity: false,
        frozenStore,
      },
      filesIndexFile,
      {
        expectedPkg: { name: peekOpts.name, version: peekOpts.version },
      }
    ).then(({ bundledManifest }) => {
      if (!bundledManifest) return undefined
      return bundledManifest as DependencyManifest
    }).catch(() => undefined)
    peekLockerForPeek.set(filesIndexFile, request)
    return request
  }
}

/**
 * Construct the LRU `PackageMetaCache` instance the resolver uses by
 * default. Exported so the install layer can build one cache and hand
 * the same reference to both the resolver and the verifier — the
 * verifier's fast path reads from it when the resolver has already
 * fetched a packument during the same install.
 */
export function createDefaultPackageMetaCache (): PackageMetaCache {
  return new LRUCache<string, PackageMeta>({
    max: 10000,
    ttl: 120 * 1000, // 2 minutes
  })
}
