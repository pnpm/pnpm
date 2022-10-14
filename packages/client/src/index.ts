import {
  createResolver as _createResolver,
  ResolveFunction,
  ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { FetchFromRegistry, GetCredentials, RetryTimeoutOptions } from '@pnpm/fetching-types'
import type { CustomFetchers, GitFetcher, DirectoryFetcher } from '@pnpm/fetcher-base'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createTarballFetcher, TarballFetchers } from '@pnpm/tarball-fetcher'
import getCredentialsByURI from 'credentials-by-uri'
import mem from 'mem'

export { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  customFetchers?: CustomFetchers
  retry?: RetryTimeoutOptions
  timeout?: number
  userAgent?: string
  userConfig?: Record<string, string>
  gitShallowHosts?: string[]
} & ResolverFactoryOptions & AgentOptions

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.authConfig, registry, opts.userConfig))
  return {
    fetchers: createFetchers(fetchFromRegistry, getCredentials, opts, opts.customFetchers),
    resolve: _createResolver(fetchFromRegistry, getCredentials, opts),
  }
}

export function createResolver (opts: ClientOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.authConfig, registry))
  return _createResolver(fetchFromRegistry, getCredentials, opts)
}

type Fetchers = {
  git: GitFetcher
  directory: DirectoryFetcher
} & TarballFetchers

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  opts: Pick<ClientOptions, 'retry' | 'gitShallowHosts'>,
  customFetchers?: CustomFetchers
): Fetchers {
  const defaultFetchers = {
    ...createTarballFetcher(fetchFromRegistry, getCredentials, opts),
    ...createGitFetcher(opts),
    ...createDirectoryFetcher(),
  }

  const overwrites = Object.entries(customFetchers ?? {})
    .reduce((acc, [fetcherName, factory]) => {
      acc[fetcherName] = factory({ defaultFetchers })
      return acc
    }, {})

  return {
    ...defaultFetchers,
    ...overwrites,
  }
}
