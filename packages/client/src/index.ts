import createResolve, { ResolverFactoryOptions } from '@pnpm/default-resolver'
import {
  createFetchFromRegistry,
  FetchFromRegistry,
  RetryTimeoutOptions,
} from '@pnpm/fetch'
import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'

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
  fetchFromRegistry: FetchFromRegistry,
  opts: {
    alwaysAuth?: boolean,
    rawConfig: object,
    retry?: RetryTimeoutOptions,
  }
) {
  return {
    ...createTarballFetcher(fetchFromRegistry, opts),
    ...fetchFromGit(),
  }
}
