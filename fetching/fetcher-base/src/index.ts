import '@total-typescript/ts-reset'
import type {
  Resolution,
  GitResolution,
  DirectoryResolution,
} from '@pnpm/resolver-base'
import type { Cafs } from '@pnpm/cafs-types'
import type { DependencyManifest } from '@pnpm/types'

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface FetchOptions {
  filesIndexFile: string
  lockfileDir: string
  onStart?: ((totalSize: number | null, attempt: number) => void) | undefined
  onProgress?: ((downloaded: number) => void | undefined)
  readManifest?: boolean | undefined
  pkg: PkgNameVersion
}

export type FetchFunction<
  FetcherResolution = Resolution,
  Options = FetchOptions,
  Result = FetchResult,
> = (
  cafs: Cafs,
  resolution: FetcherResolution,
  opts: Options
) => Promise<Result>

export interface FetchResult {
  local?: boolean | undefined
  manifest?: DependencyManifest | undefined
  filesIndex: Record<string, string>
}

export interface GitFetcherOptions {
  readManifest?: boolean | undefined
  filesIndexFile: string
  pkg?: PkgNameVersion | undefined
}

export type GitFetcher = FetchFunction<
  GitResolution,
  GitFetcherOptions,
  { filesIndex: Record<string, string>; manifest?: DependencyManifest | undefined }
>

export interface DirectoryFetcherOptions {
  lockfileDir: string | undefined
  readManifest?: boolean | undefined
}

export interface DirectoryFetcherResult {
  local: true
  filesIndex: Record<string, string>
  packageImportMethod: 'hardlink'
  manifest?: DependencyManifest | undefined
}

export type DirectoryFetcher = FetchFunction<
  DirectoryResolution,
  DirectoryFetcherOptions,
  DirectoryFetcherResult
>

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
