import {
  createResolver as _createResolver,
  type ResolveFunction,
  type ResolverFactoryOptions,
} from '@pnpm/default-resolver'
import { type AgentOptions, createFetchFromRegistry } from '@pnpm/fetch'
import { type SslConfig } from '@pnpm/types'
import { type FetchFromRegistry, type GetAuthHeader, type RetryTimeoutOptions } from '@pnpm/fetching-types'
import type { CustomFetchers, GitFetcher, DirectoryFetcher, NodeRuntimeFetcher, DenoRuntimeFetcher } from '@pnpm/fetcher-base'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
import { createDenoRuntimeFetcher } from '@pnpm/runtime.deno-installer'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { createTarballFetcher, type TarballFetchers } from '@pnpm/tarball-fetcher'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createNodeRuntimeFetcher } from '@pnpm/node.fetcher'
import mapValues from 'ramda/src/map'

export type { ResolveFunction }

export type ClientOptions = {
  authConfig: Record<string, string>
  customFetchers?: CustomFetchers
  ignoreScripts?: boolean
  rawConfig: Record<string, string>
  sslConfigs?: Record<string, SslConfig>
  retry?: RetryTimeoutOptions
  timeout?: number
  unsafePerm?: boolean
  userAgent?: string
  userConfig?: Record<string, string>
  gitShallowHosts?: string[]
  resolveSymlinksInInjectedDirs?: boolean
  includeOnlyPackageFiles?: boolean
  preserveAbsolutePaths?: boolean
} & ResolverFactoryOptions & AgentOptions

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
  clearResolutionCache: () => void
}

export function createClient (opts: ClientOptions): Client {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })
  const { resolve, clearCache: clearResolutionCache } = _createResolver(fetchFromRegistry, getAuthHeader, opts)
  return {
    fetchers: createFetchers(fetchFromRegistry, getAuthHeader, opts, opts.customFetchers),
    resolve,
    clearResolutionCache,
  }
}

export function createResolver (opts: ClientOptions): { resolve: ResolveFunction, clearCache: () => void } {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.authConfig, userSettings: opts.userConfig })
  return _createResolver(fetchFromRegistry, getAuthHeader, opts)
}

type Fetchers = {
  git: GitFetcher
  directory: DirectoryFetcher
  nodeRuntime: NodeRuntimeFetcher
  denoRuntime: DenoRuntimeFetcher
} & TarballFetchers

function createFetchers (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: Pick<ClientOptions, 'rawConfig' | 'retry' | 'gitShallowHosts' | 'resolveSymlinksInInjectedDirs' | 'unsafePerm' | 'includeOnlyPackageFiles' | 'offline'>,
  customFetchers?: CustomFetchers
): Fetchers {
  const tarballFetchers = createTarballFetcher(fetchFromRegistry, getAuthHeader, opts)
  const defaultFetchers = {
    ...tarballFetchers,
    ...createGitFetcher(opts),
    ...createDirectoryFetcher({ resolveSymlinks: opts.resolveSymlinksInInjectedDirs, includeOnlyPackageFiles: opts.includeOnlyPackageFiles }),
    ...createNodeRuntimeFetcher({
      fetch: fetchFromRegistry,
      fetchFromRemoteTarball: tarballFetchers.remoteTarball,
      offline: opts.offline,
      rawConfig: opts.rawConfig,
    }),
    ...createDenoRuntimeFetcher({
      fetch: fetchFromRegistry,
      offline: opts.offline,
      rawConfig: opts.rawConfig,
    }),
  }

  const overwrites = mapValues(
    (factory: any) => factory({ defaultFetchers }), // eslint-disable-line @typescript-eslint/no-explicit-any
    customFetchers ?? {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
  )

  return {
    ...defaultFetchers,
    ...overwrites,
  }
}
