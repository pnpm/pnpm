import {
  createResolver as _createResolver,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { type DispatcherOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { type SslConfig } from '@pnpm/types'
import { type CustomResolver, type CustomFetcher } from '@pnpm/hooks.types'
import { type FetchFromRegistry, type GetAuthHeader, type RetryTimeoutOptions } from '@pnpm/fetching-types'
import type { GitFetcher, DirectoryFetcher, BinaryFetcher } from '@pnpm/fetcher-base'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createTarballFetcher, type TarballFetchers } from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createBinaryFetcher } from '@pnpm/fetching.binary-fetcher'

export type { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  customResolvers?: CustomResolver[]
  customFetchers?: CustomFetcher[]
  ignoreScripts?: boolean
  rawConfig: Record<string, string>
  sslConfigs?: Record<string, SslConfig>
  retry?: RetryTimeoutOptions
  timeout?: number
  unsafePerm?: boolean
  userAgent?: string
  userConfig?: Record<string, string>
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
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })

  const { resolve, clearCache: clearResolutionCache } = _createResolver(fetchFromRegistry, getAuthHeader, { ...opts, customResolvers: opts.customResolvers })
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts),
    resolve,
    clearResolutionCache,
  }
}

export function createResolver (opts: ClientOptions): { resolve: ResolveFunction, clearCache: () => void } {
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
  opts: Pick<ClientOptions, 'rawConfig' | 'retry' | 'gitShallowHosts' | 'resolveSymlinksInInjectedDirs' | 'unsafePerm' | 'includeOnlyPackageFiles' | 'offline' | 'fetchMinSpeedKiBps'>
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
    }),
  }
}
