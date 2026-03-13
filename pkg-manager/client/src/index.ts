import {
  createResolver as _createResolver,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { type AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import type { BinaryFetcher, DirectoryFetcher, GitFetcher } from '@pnpm/fetcher-base'
import { createBinaryFetcher } from '@pnpm/fetching.binary-fetcher'
import type { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching-types'
import { createGitFetcher } from '@pnpm/git-fetcher'
import type { CustomFetcher, CustomResolver } from '@pnpm/hooks.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { StoreIndex } from '@pnpm/store.index'
import { createTarballFetcher, type TarballFetchers } from '@pnpm/tarball-fetcher'
import type { SslConfig } from '@pnpm/types'

export type { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  customResolvers?: CustomResolver[]
  customFetchers?: CustomFetcher[]
  ignoreScripts?: boolean
  rawConfig: Record<string, string>
  sslConfigs?: Record<string, SslConfig>
  retry?: RetryTimeoutOptions
  storeIndex: StoreIndex
  timeout?: number
  unsafePerm?: boolean
  userAgent?: string
  userConfig?: Record<string, string>
  gitShallowHosts?: string[]
  resolveSymlinksInInjectedDirs?: boolean
  includeOnlyPackageFiles?: boolean
  preserveAbsolutePaths?: boolean
  fetchMinSpeedKiBps?: number
} & ResolverFactoryOptions & AgentOptions

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
  clearResolutionCache: () => void
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })

  const { resolve, clearCache: clearResolutionCache } = _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, customResolvers: opts.customResolvers })
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts),
    resolve,
    clearResolutionCache,
  }
}

export function createResolver (opts: Omit<ClientOptions, 'storeIndex'>): { resolve: ResolveFunction, clearCache: () => void } {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })

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
  opts: Pick<ClientOptions, 'rawConfig' | 'retry' | 'gitShallowHosts' | 'resolveSymlinksInInjectedDirs' | 'unsafePerm' | 'includeOnlyPackageFiles' | 'offline' | 'fetchMinSpeedKiBps' | 'storeIndex'>
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
      rawConfig: opts.rawConfig,
      storeIndex: opts.storeIndex,
    }),
  }
}
