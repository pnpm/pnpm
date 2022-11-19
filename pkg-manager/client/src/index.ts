import {
  createResolver as _createResolver,
  ResolveFunction,
  ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching-types'
import type { CustomFetchers, GitFetcher, DirectoryFetcher } from '@pnpm/fetcher-base'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createTarballFetcher, TarballFetchers } from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import mapValues from 'ramda/src/map'

export { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  customFetchers?: CustomFetchers
  retry?: RetryTimeoutOptions
  timeout?: number
  userAgent?: string
  userConfig?: Record<string, string>
  gitShallowHosts?: string[]
  resolveSymlinksInInjectedDirs?: boolean
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

export function createResolver (opts: ClientOptions) {
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
  opts: Pick<ClientOptions, 'retry' | 'gitShallowHosts' | 'resolveSymlinksInInjectedDirs'>,
  customFetchers?: CustomFetchers
): Fetchers {
  const defaultFetchers = {
    ...createTarballFetcher(fetchFromRegistry, getAuthHeader, opts),
    ...createGitFetcher(opts),
    ...createDirectoryFetcher({ resolveSymlinks: opts.resolveSymlinksInInjectedDirs }),
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
