import '@total-typescript/ts-reset'
import type {
  Fetchers,
  FetchFunction,
  DirectoryFetcher,
  GitFetcher,
} from '@pnpm/resolver-base'

interface CustomFetcherFactoryOptions {
  defaultFetchers: Fetchers
}

export type CustomFetcherFactory<Fetcher> = (
  opts: CustomFetcherFactoryOptions
) => Fetcher

export interface CustomFetchers {
  localTarball?: CustomFetcherFactory<FetchFunction> | undefined
  remoteTarball?: CustomFetcherFactory<FetchFunction> | undefined
  gitHostedTarball?: CustomFetcherFactory<FetchFunction> | undefined
  directory?: CustomFetcherFactory<DirectoryFetcher> | undefined
  git?: CustomFetcherFactory<GitFetcher> | undefined
}
