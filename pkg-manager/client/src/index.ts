import '@total-typescript/ts-reset'
import {
  createResolver as _createResolver,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { type AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import {
  type FetchFromRegistry,
  type GetAuthHeader,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createGitFetcher } from '@pnpm/git-fetcher'
import {
  createTarballFetcher,
  type TarballFetchers,
} from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import mapValues from 'ramda/src/map'
import { ClientOptions, Client, GitFetcher, DirectoryFetcher, CustomFetchers } from '@pnpm/types'

export type { ResolveFunction }

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

type Fetchers = {
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
    (factory: any) => factory({ defaultFetchers }), // eslint-disable-line @typescript-eslint/no-explicit-any
    customFetchers ?? ({} as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  )

  return {
    ...defaultFetchers,
    ...overwrites,
  }
}
