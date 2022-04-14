import createResolve, {
  ResolveFunction,
  ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { FetchFromRegistry, GetCredentials, RetryTimeoutOptions } from '@pnpm/fetching-types'
import createDirectoryFetcher from '@pnpm/directory-fetcher'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import getCredentialsByURI from 'credentials-by-uri'
import mem from 'mem'

export { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  retry?: RetryTimeoutOptions
  timeout?: number
  userAgent?: string
  userConfig?: Record<string, string>
  gitShallowHosts?: string[]
} & ResolverFactoryOptions & AgentOptions

export default function (opts: ClientOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.authConfig, registry, opts.userConfig))
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
  opts: Pick<ClientOptions, 'retry' | 'gitShallowHosts'>
) {
  return {
    ...createTarballFetcher(fetchFromRegistry, getCredentials, opts),
    ...fetchFromGit(opts),
    ...createDirectoryFetcher(),
  }
}
