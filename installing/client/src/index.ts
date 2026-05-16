import { NODE_EXTRAS_IGNORE_PATTERN } from '@pnpm/engine.runtime.node-resolver'
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
  createResolutionVerifier,
  createResolver as _createResolver,
  type ResolutionVerifierFactoryOptions,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/resolving.default-resolver'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { StoreIndex } from '@pnpm/store.index'
import type { RegistryConfig } from '@pnpm/types'

export type { ResolutionVerifier, ResolveFunction }

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
  & Pick<ResolutionVerifierFactoryOptions, 'minimumReleaseAge' | 'minimumReleaseAgeStrict' | 'minimumReleaseAgeExclude'>

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
  clearResolutionCache: () => void
  /**
   * Combined verifier across the resolver chain. `undefined` when no
   * resolver-level policy is active (today: minimumReleaseAge strict mode).
   * Used by the install layer to re-validate an already-resolved lockfile
   * entry without re-doing resolution.
   */
  verifyResolution?: ResolutionVerifier
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri, opts.registries?.default)

  const { resolve, clearCache: clearResolutionCache } = _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, customResolvers: opts.customResolvers })
  const verifyResolution = createResolutionVerifier(fetchFromRegistry, opts)
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts),
    resolve,
    clearResolutionCache,
    verifyResolution,
  }
}

export function createResolver (opts: Omit<ClientOptions, 'storeIndex'>): { resolve: ResolveFunction, clearCache: () => void } {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri, opts.registries?.default)

  return _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, customResolvers: opts.customResolvers })
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
