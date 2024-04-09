import {
  createResolver as _createResolver,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { type AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { type SslConfig } from '@pnpm/types'
import { type FetchFromRegistry, type GetAuthHeader, type RetryTimeoutOptions } from '@pnpm/fetching-types'
import type { CustomFetchers, GitFetcher, DirectoryFetcher } from '@pnpm/fetcher-base'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createTarballFetcher, type TarballFetchers } from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import mapValues from 'ramda/src/map'

export type { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  customFetchers?: CustomFetchers
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
} & ResolverFactoryOptions & AgentOptions

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts, opts.customFetchers),
    resolve: _createResolver(fetchFromRegistry, getAuthHeader, opts),
  }
}

export function createResolver (opts: ClientOptions): ResolveFunction {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })
  return _createResolver(fetchFromRegistry, getAuthHeader, opts)
}

type Fetchers = {
  git: GitFetcher
  directory: DirectoryFetcher
} & TarballFetchers

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: Pick<ClientOptions, 'rawConfig' | 'retry' | 'gitShallowHosts' | 'resolveSymlinksInInjectedDirs' | 'unsafePerm' | 'includeOnlyPackageFiles'>,
  customFetchers?: CustomFetchers
): Fetchers {
  const defaultFetchers = {
    ...createTarballFetcher(fetchFromRegistry, getAuthHeader, opts),
    ...createGitFetcher(opts),
    ...createDirectoryFetcher({ resolveSymlinks: opts.resolveSymlinksInInjectedDirs, includeOnlyPackageFiles: opts.includeOnlyPackageFiles }),
  }

  const overwrites = mapValues(
    (factory: any) => factory({ defaultFetchers }), // eslint-disable-line @typescript-eslint/no-explicit-any
    customFetchers ?? {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
  )

  return {
    ...defaultFetchers,
    ...overwrites,
  }
}
