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
  createResolver as _createResolver,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/resolving.default-resolver'
import type { StoreIndex } from '@pnpm/store.index'
import type { RegistryConfig } from '@pnpm/types'

export type { ResolveFunction }

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

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
  clearResolutionCache: () => void
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri, opts.registries?.default)

  const { resolve, clearCache: clearResolutionCache } = _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, customResolvers: opts.customResolvers })
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts),
    resolve,
    clearResolutionCache,
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
