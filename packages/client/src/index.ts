import createResolve, { ResolverFactoryOptions } from '@pnpm/default-resolver'
import {
  createFetchFromRegistry,
  FetchFromRegistry,
  RetryTimeoutOptions,
} from '@pnpm/fetch'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')

export default function (opts: {
  alwaysAuth?: boolean,
  ca?: string,
  cert?: string,
  key?: string,
  localAddress?: string,
  proxy?: string,
  rawConfig: object,
  retry?: RetryTimeoutOptions,
  strictSSL?: boolean,
  userAgent?: string,
} & ResolverFactoryOptions) {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.rawConfig, registry))
  return {
    fetchers: createFetchers(fetchFromRegistry, getCredentials, opts),
    resolve: createResolve(fetchFromRegistry, getCredentials, opts),
  }
}

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: (registry: string) => {
    authHeaderValue: string | undefined,
    alwaysAuth: boolean | undefined,
  },
  opts: {
    alwaysAuth?: boolean,
    rawConfig: object,
    retry?: RetryTimeoutOptions,
  }
) {
  return {
    ...createTarballFetcher(fetchFromRegistry, getCredentials, opts),
    ...fetchFromGit(),
  }
}
