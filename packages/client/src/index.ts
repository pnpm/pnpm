import createResolve, {
  ResolveFunction,
  ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { FetchFromRegistry, GetCredentials, RetryTimeoutOptions } from '@pnpm/fetching-types'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')

export { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  retry?: RetryTimeoutOptions
  userAgent?: string
} & ResolverFactoryOptions & AgentOptions

export default function (opts: ClientOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.authConfig, registry))
  return {
    fetchers: createFetchers(fetchFromRegistry, getCredentials, opts),
    resolve: createResolve(fetchFromRegistry, getCredentials, opts),
  }
}

export function createResolver (opts: ClientOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.authConfig, registry))
  return createResolve(fetchFromRegistry, getCredentials, opts)
}

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  opts: {
    retry?: RetryTimeoutOptions
  }
) {
  return {
    ...createTarballFetcher(fetchFromRegistry, getCredentials, opts),
    ...fetchFromGit(),
  }
}
