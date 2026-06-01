import { NODE_EXTRAS_IGNORE_PATTERN } from '@pnpm/engine.runtime.node-resolver'
import { PnpmError } from '@pnpm/error'
import { createBinaryFetcher } from '@pnpm/fetching.binary-fetcher'
import { createDirectoryFetcher } from '@pnpm/fetching.directory-fetcher'
import type { BinaryFetcher, DirectoryFetcher, GitFetcher } from '@pnpm/fetching.fetcher-base'
import { createGitFetcher } from '@pnpm/fetching.git-fetcher'
import { createTarballFetcher, type TarballFetchers } from '@pnpm/fetching.tarball-fetcher'
import type { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching.types'
import type { CustomFetcher, CustomResolver } from '@pnpm/hooks.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type DispatcherOptions } from '@pnpm/network.fetch'
import {
  createDefaultPackageMetaCache,
  createResolutionVerifiers,
  createResolver as _createResolver,
  type ResolutionVerifierFactoryOptions,
  type ResolveFunction,
  type ResolveLatestDispatcher,
  type ResolverFactoryOptions,
} from '@pnpm/resolving.default-resolver'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE } from '@pnpm/resolving.npm-resolver'
import type { LatestInfo, LatestQuery, ResolutionPolicyViolation, ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { StoreIndex } from '@pnpm/store.index'
import type { RegistryConfig } from '@pnpm/types'

export type { LatestInfo, LatestQuery, ResolutionVerifier, ResolveFunction, ResolveLatestDispatcher }

export type ClientOptions = {
  configByUri: Record<string, RegistryConfig>
  customResolvers?: CustomResolver[]
  customFetchers?: CustomFetcher[]
  ignoreScripts?: boolean
  retry?: RetryTimeoutOptions
  storeIndex: StoreIndex
  timeout?: number
  nodeDownloadMirrors?: Record<string, string>
  unsafePerm?: boolean
  userAgent?: string
  gitShallowHosts?: string[]
  resolveSymlinksInInjectedDirs?: boolean
  includeOnlyPackageFiles?: boolean
  preserveAbsolutePaths?: boolean
  fetchMinSpeedKiBps?: number
} & ResolverFactoryOptions & DispatcherOptions
  & Pick<ResolutionVerifierFactoryOptions,
  | 'minimumReleaseAge'
  | 'minimumReleaseAgeStrict'
  | 'minimumReleaseAgeExclude'
  | 'ignoreMissingTimeField'
  | 'trustPolicy'
  | 'trustPolicyExclude'
  | 'trustPolicyIgnoreAfter'
  >

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
  clearResolutionCache: () => void
  /**
   * List of resolver-side verifiers — one entry per active policy
   * (today: at most one, `npm.minimumReleaseAge`). Empty when no policy
   * is active. The install layer fans out across the list to re-validate
   * each lockfile entry; each verifier handles its own protocol
   * short-circuit inside `verify`.
   */
  resolutionVerifiers: ResolutionVerifier[]
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri)

  // One per-install LRU shared with both the resolver's pickPackage
  // pass and the verifier's lookup chain. When the resolver populates
  // an entry for a given `name`, a later verify of the same name
  // (e.g. the post-resolution gate, or a second `mutateModules` call
  // in the same long-lived process) reuses it instead of re-fetching.
  const metaCache = createDefaultPackageMetaCache()
  const { resolve, clearCache: clearResolutionCache } = _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, metaCache, customResolvers: opts.customResolvers })
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts),
    resolve,
    clearResolutionCache,
    resolutionVerifiers: createResolutionVerifiers(fetchFromRegistry, { ...opts, metaCache }),
  }
}

export function createResolver (opts: Omit<ClientOptions, 'storeIndex'>): { resolve: ResolveFunction, resolveLatest: ResolveLatestDispatcher, clearCache: () => void } {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri)

  return _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, customResolvers: opts.customResolvers })
}

/**
 * Wraps a `ResolveFunction` so any inline policy violation surfaced by
 * the resolver is rethrown as a `PnpmError` instead of being returned on
 * the result. Use this from one-shot callers (dlx, self-update) that
 * have nowhere to defer a violation to — the install command leaves
 * resolution unwrapped because it aggregates violations across the
 * whole tree before deciding what to do.
 *
 * The error mapping is centralized here so future violation codes
 * (today: `MINIMUM_RELEASE_AGE_VIOLATION`) get a consistent error code
 * across every strict-mode caller without each call site re-translating.
 */
export function makeResolutionStrict (resolve: ResolveFunction): ResolveFunction {
  return (async (wantedDependency, opts) => {
    const result = await resolve(wantedDependency, opts)
    if (result?.policyViolation) {
      throw policyViolationToError(result.policyViolation)
    }
    return result
  }) as ResolveFunction
}

function policyViolationToError (violation: ResolutionPolicyViolation): PnpmError {
  const message = `${violation.name}@${violation.version} ${violation.reason}`
  // Map the per-violation `code` to the user-facing PnpmError code that
  // pre-refactor callers (and `default-reporter`) already recognize.
  // Future violation codes get their mapping added here so call sites
  // don't have to re-translate.
  const errorCode = violation.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE
    ? 'NO_MATURE_MATCHING_VERSION'
    : violation.code
  return new PnpmError(errorCode, message)
}

type Fetchers = {
  git: GitFetcher
  directory: DirectoryFetcher
  binary: BinaryFetcher
} & TarballFetchers

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: Pick<ClientOptions, 'retry' | 'gitShallowHosts' | 'resolveSymlinksInInjectedDirs' | 'unsafePerm' | 'userAgent' | 'includeOnlyPackageFiles' | 'offline' | 'fetchMinSpeedKiBps' | 'storeIndex'>
): Fetchers {
  const tarballFetchers = createTarballFetcher(fetchFromRegistry, getAuthHeader, opts)
  return {
    ...tarballFetchers,
    ...createGitFetcher(opts),
    ...createDirectoryFetcher({ resolveSymlinks: opts.resolveSymlinksInInjectedDirs, includeOnlyPackageFiles: opts.includeOnlyPackageFiles }),
    ...createBinaryFetcher({
      fetch: fetchFromRegistry,
      fetchFromRemoteTarball: tarballFetchers.remoteTarball,
      offline: opts.offline,
      storeIndex: opts.storeIndex,
      archiveFilters: { node: NODE_EXTRAS_IGNORE_PATTERN },
    }),
  }
}
