import '@total-typescript/ts-reset'

import mapValues from 'ramda/src/map'

import {
  createResolver as _createResolver,
} from '@pnpm/default-resolver'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import {
  createTarballFetcher,
  type TarballFetchers,
} from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type{ ClientOptions, Client, GitFetcher, DirectoryFetcher, CustomFetchers, ResolveFunction, FetchFromRegistry, GetAuthHeader } from '@pnpm/types'

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

export type Fetchers = {
  git: GitFetcher
  directory: DirectoryFetcher
} & TarballFetchers

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
  customFetchers?: CustomFetchers
): Fetchers {
  const defaultFetchers = {
    ...createTarballFetcher(fetchFromRegistry, getAuthHeader, opts),
    ...createGitFetcher(opts),
    ...createDirectoryFetcher({
      resolveSymlinks: opts.resolveSymlinksInInjectedDirs,
      includeOnlyPackageFiles: opts.includeOnlyPackageFiles,
    }),
  }

  const overwrites = mapValues(
    (factory: any) => {
      return factory({ defaultFetchers });
    },
    customFetchers ?? {}
  )

  return {
    ...defaultFetchers,
    ...overwrites,
  }
}
