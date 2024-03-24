import '@total-typescript/ts-reset'

import mapValues from 'ramda/src/map'

import type {
  Client,
  Fetchers,
  GetAuthHeader,
  ClientOptions,
  CustomFetchers,
  ResolveFunction,
  FetchFromRegistry,
} from '@pnpm/types'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createResolver as _createResolver } from '@pnpm/default-resolver'

export function createClient(opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const getAuthHeader = createGetAuthHeaderByURI({
    allSettings: opts.authConfig,
    userSettings: opts.userConfig,
  })

  return {
    fetchers: createFetchers(
      fetchFromRegistry,
      getAuthHeader,
      opts,
      opts.customFetchers
    ),
    resolve: _createResolver(fetchFromRegistry, getAuthHeader, opts),
  }
}

export function createResolver(opts: ClientOptions): ResolveFunction {
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const getAuthHeader = createGetAuthHeaderByURI({
    allSettings: opts.authConfig,
    userSettings: opts.userConfig,
  })

  return _createResolver(fetchFromRegistry, getAuthHeader, opts)
}

function createFetchers(
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: Pick<
    ClientOptions,
    | 'rawConfig'
    | 'retry'
    | 'gitShallowHosts'
    | 'resolveSymlinksInInjectedDirs'
    | 'unsafePerm'
    | 'includeOnlyPackageFiles'
  >,
  customFetchers?: CustomFetchers | undefined
): Fetchers {
  const defaultFetchers = {
    ...createTarballFetcher(fetchFromRegistry, getAuthHeader, opts),
    ...createGitFetcher(opts),
    ...createDirectoryFetcher({
      resolveSymlinks: opts.resolveSymlinksInInjectedDirs,
      includeOnlyPackageFiles: opts.includeOnlyPackageFiles,
    }),
  } satisfies Fetchers

  const overwrites = mapValues(
    (factory) => {
      return factory?.({ defaultFetchers });
    },
    // @ts-ignore
    customFetchers ?? {}
  ) as Fetchers

  return {
    ...defaultFetchers,
    ...overwrites,
  }
}
