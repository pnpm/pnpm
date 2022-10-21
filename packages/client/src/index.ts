import createResolve, {
  ResolveFunction,
  ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching-types'
import createDirectoryFetcher from '@pnpm/directory-fetcher'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'

export { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  retry?: RetryTimeoutOptions
  timeout?: number
  userAgent?: string
  userConfig?: Record<string, string>
} & ResolverFactoryOptions & AgentOptions

export default function (opts: ClientOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts),
    resolve: createResolve(fetchFromRegistry, getAuthHeader, opts),
  }
}

export function createResolver (opts: ClientOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })
  return createResolve(fetchFromRegistry, getAuthHeader, opts)
}

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: {
    retry?: RetryTimeoutOptions
  }
) {
  return {
    ...createTarballFetcher(fetchFromRegistry, getAuthHeader, opts),
    ...fetchFromGit(),
    ...createDirectoryFetcher(),
  }
}
