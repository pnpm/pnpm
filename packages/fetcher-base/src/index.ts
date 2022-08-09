import { Resolution, GitResolution, DirectoryResolution } from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import { IntegrityLike } from 'ssri'

export interface PackageFileInfo {
  checkedAt?: number // Nullable for backward compatibility
  integrity: string
  mode: number
  size: number
}

export type PackageFilesResponse = {
  fromStore: boolean
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
} & ({
  local: true
  filesIndex: Record<string, string>
} | {
  local?: false
  filesIndex: Record<string, PackageFileInfo>
})

export interface ImportPackageOpts {
  requiresBuild?: boolean
  sideEffectsCacheKey?: string
  filesResponse: PackageFilesResponse
  force: boolean
}

export type ImportPackageFunction = (
  to: string,
  opts: ImportPackageOpts
) => Promise<{ isBuilt: boolean, importMethod: undefined | string }>

export interface Cafs {
  addFilesFromDir: (dir: string, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  addFilesFromTarball: (stream: NodeJS.ReadableStream, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}

export interface FetchOptions {
  manifest?: DeferredManifestPromise
  lockfileDir: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
}

export interface DeferredManifestPromise {
  resolve: (manifest: DependencyManifest | undefined) => void
  reject: (err: Error) => void
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

export interface FileWriteResult {
  checkedAt: number
  integrity: IntegrityLike
}

export interface FilesIndex {
  [filename: string]: {
    mode: number
    size: number
    writeResult: Promise<FileWriteResult>
  }
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
