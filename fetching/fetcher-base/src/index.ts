import { Resolution, GitResolution, DirectoryResolution } from '@pnpm/resolver-base'
import type { DeferredManifestPromise, Cafs, FilesIndex } from '@pnpm/cafs-types'

export interface FetchOptions {
  manifest?: DeferredManifestPromise
  lockfileDir: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
}

export type FetchFunction<FetcherResolution = Resolution, Options = FetchOptions, Result = FetchResult> = (
  cafs: Cafs,
  resolution: FetcherResolution,
  opts: Options
) => Promise<Result>

export type FetchResult = {
  local?: false
  filesIndex: FilesIndex
} | {
  local: true
  filesIndex: Record<string, string>
}

export interface GitFetcherOptions {
  manifest?: DeferredManifestPromise
}

export type GitFetcher = FetchFunction<GitResolution, GitFetcherOptions, { filesIndex: FilesIndex }>

export interface DirectoryFetcherOptions {
  lockfileDir: string
  manifest?: DeferredManifestPromise
}

export interface DirectoryFetcherResult {
  local: true
  filesIndex: Record<string, string>
  packageImportMethod: 'hardlink'
}

export type DirectoryFetcher = FetchFunction<DirectoryResolution, DirectoryFetcherOptions, DirectoryFetcherResult>

export interface Fetchers {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
  directory: DirectoryFetcher
  git: GitFetcher
}

interface CustomFetcherFactoryOptions {
  defaultFetchers: Fetchers
}

export type CustomFetcherFactory<Fetcher> = (opts: CustomFetcherFactoryOptions) => Fetcher

export interface CustomFetchers {
  localTarball?: CustomFetcherFactory<FetchFunction>
  remoteTarball?: CustomFetcherFactory<FetchFunction>
  gitHostedTarball?: CustomFetcherFactory<FetchFunction>
  directory?: CustomFetcherFactory<DirectoryFetcher>
  git?: CustomFetcherFactory<GitFetcher>
}
