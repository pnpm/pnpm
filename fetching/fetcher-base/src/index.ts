import { type Resolution, type GitResolution, type DirectoryResolution } from '@pnpm/resolver-base'
import { type Cafs } from '@pnpm/cafs-types'
import { type DependencyManifest } from '@pnpm/types'

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface FetchOptions {
  filesIndexFile: string
  lockfileDir: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
  readManifest?: boolean
  pkg: PkgNameVersion
}

export type FetchFunction<FetcherResolution = Resolution, Options = FetchOptions, Result = FetchResult> = (
  cafs: Cafs,
  resolution: FetcherResolution,
  opts: Options
) => Promise<Result>

export interface FetchResult {
  local?: boolean
  manifest?: DependencyManifest
  filesIndex: Record<string, string>
  requiresBuild: boolean
}

export interface GitFetcherOptions {
  readManifest?: boolean
  filesIndexFile: string
  pkg?: PkgNameVersion
}

export interface GitFetcherResult {
  filesIndex: Record<string, string>
  manifest?: DependencyManifest
  requiresBuild: boolean
}

export type GitFetcher = FetchFunction<GitResolution, GitFetcherOptions, GitFetcherResult>

export interface DirectoryFetcherOptions {
  lockfileDir: string
  readManifest?: boolean
}

export interface DirectoryFetcherResult {
  local: true
  filesIndex: Record<string, string>
  packageImportMethod: 'hardlink'
  manifest?: DependencyManifest
  requiresBuild: boolean
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
