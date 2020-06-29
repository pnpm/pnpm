import createResolve, { ResolverFactoryOptions } from '@pnpm/default-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { FetchFromRegistry, GetCredentials, RetryTimeoutOptions } from '@pnpm/fetching-types'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')

export default function (opts: {
  ca?: string,
  cert?: string,
  key?: string,
  localAddress?: string,
  proxy?: string,
  authConfig: Record<string, string>,
  retry?: RetryTimeoutOptions,
  strictSSL?: boolean,
  userAgent?: string,
} & ResolverFactoryOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.authConfig, registry))
  return {
    fetchers: createFetchers(fetchFromRegistry, getCredentials, opts),
    resolve: createResolve(fetchFromRegistry, getCredentials, opts),
  }
}

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  opts: {
    retry?: RetryTimeoutOptions,
  }
) {
  return {
    ...createTarballFetcher(fetchFromRegistry, getCredentials, opts),
    ...fetchFromGit(),
  }
}
