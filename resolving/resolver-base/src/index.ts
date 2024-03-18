import '@total-typescript/ts-reset'
import type { DependencyManifest } from '@pnpm/types'
import type { Cafs } from '@pnpm/cafs-types'

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

export interface FetchResult {
  local?: boolean | undefined
  manifest?: DependencyManifest | undefined
  filesIndex: Record<string, string>
}

export interface DirectoryFetcherOptions {
  lockfileDir: string | undefined
  readManifest?: boolean | undefined
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

export interface Fetchers {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
  directory: DirectoryFetcher
  git: GitFetcher
}
/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: keyof Fetchers | undefined
  tarball: string
  integrity?: string
}

/**
 * directory on a file system
 */
export interface DirectoryResolution {
  type: 'directory'
  directory: string
}

export interface GitResolution {
  commit: string
  repo: string
  type: 'git'
}

export type Resolution =
  | TarballResolution
  | DirectoryResolution
  | GitResolution
  | ({ type: keyof Fetchers } & object)

export interface ResolveResult {
  id: string
  latest?: string | undefined
  publishedAt?: string | undefined
  manifest?: DependencyManifest | undefined
  normalizedPref?: string | undefined // is null for npm-hosted dependencies
  resolution: Resolution
  resolvedVia:
    | 'npm-registry'
    | 'git-repository'
    | 'local-filesystem'
    | 'url'
    | string
}

export interface WorkspacePackages {
  [name: string]: {
    [version: string]: {
      dir: string
      manifest: DependencyManifest
    }
  }
}

// This weight is set for selectors that are used on direct dependencies.
// It is important to give a bigger weight to direct dependencies.
export const DIRECT_DEP_SELECTOR_WEIGHT = 1000

export type VersionSelectorType = 'version' | 'range' | 'tag'

export interface VersionSelectors {
  [selector: string]: VersionSelectorWithWeight | VersionSelectorType
}

export interface VersionSelectorWithWeight {
  selectorType: VersionSelectorType
  weight: number
}

export interface PreferredVersions {
  [packageName: string]: VersionSelectors
}

export interface ResolveOptions {
  alwaysTryWorkspacePackages?: boolean
  defaultTag?: string
  pickLowestVersion?: boolean
  publishedBy?: Date
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean
  registry: string
  workspacePackages?: WorkspacePackages
  updateToLatest?: boolean
}

export type WantedDependency = {
  injected?: boolean
} & (
  | {
    alias?: string
    pref: string
  }
  | {
    alias: string
    pref?: string
  }
)

export type ResolveFunction = (
  wantedDependency: WantedDependency,
  opts: ResolveOptions
) => Promise<ResolveResult>
