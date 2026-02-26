import {
  type Resolution,
  type GitResolution,
  type DirectoryResolution,
  type BinaryResolution,
} from '@pnpm/resolver-base'
import { type Cafs, type FilesMap } from '@pnpm/cafs-types'
import { type AllowBuild, type BundledManifest, type DependencyManifest } from '@pnpm/types'

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface FetchOptions {
  allowBuild?: AllowBuild
  filesIndexFile: string
  lockfileDir: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
  readManifest?: boolean
  pkg: PkgNameVersion
  appendManifest?: DependencyManifest
}

export type FetchFunction<FetcherResolution = Resolution, Options = FetchOptions, Result = FetchResult> = (
  cafs: Cafs,
  resolution: FetcherResolution,
  opts: Options
) => Promise<Result>

export interface FetchResult {
  local?: boolean
  manifest?: BundledManifest
  filesMap: FilesMap
  requiresBuild: boolean
  integrity?: string
}

export interface GitFetcherOptions {
  allowBuild?: AllowBuild
  readManifest?: boolean
  filesIndexFile: string
  pkg?: PkgNameVersion
}

export interface GitFetcherResult {
  filesMap: FilesMap
  manifest?: BundledManifest
  requiresBuild: boolean
}

export type GitFetcher = FetchFunction<GitResolution, GitFetcherOptions, GitFetcherResult>

export type BinaryFetcher = FetchFunction<BinaryResolution>

export interface DirectoryFetcherOptions {
  lockfileDir: string
  readManifest?: boolean
}

export interface DirectoryFetcherResult {
  local: true
  filesMap: FilesMap
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
  binary: BinaryFetcher
}
