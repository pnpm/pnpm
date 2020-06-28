import createResolve, { ResolverFactoryOptions } from '@pnpm/default-resolver'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import createFetchFromRegistry, { FetchFromRegistry, RetryTimeoutOptions } from 'fetch-from-npm-registry'

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
  return {
    fetchers: createFetchers(fetchFromRegistry, opts),
    resolve: createResolve(fetchFromRegistry, opts),
  }
}

function createFetchers (
  fetchFromNpmRegistry: FetchFromRegistry,
  opts: {
    alwaysAuth?: boolean,
    rawConfig: object,
    retry?: RetryTimeoutOptions,
  }
) {
  return {
    ...createTarballFetcher(fetchFromNpmRegistry, opts),
    ...fetchFromGit(),
  }
}
