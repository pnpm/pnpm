import {
  type ProjectRootDir,
  type DependencyManifest,
  type PkgResolutionId,
  type PinnedVersion,
} from '@pnpm/types'

export { type PkgResolutionId }

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: undefined
  tarball: string
  integrity?: string
  path?: string
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
  path?: string
  type: 'git'
}

export type Resolution =
  TarballResolution |
  DirectoryResolution |
  GitResolution |
  ({ type: string } & object)

export type ResolvedVia =
  | 'npm-registry'
  | 'jsr-registry'
  | 'git-repository'
  | 'local-filesystem'
  | 'url'
  | 'workspace'

export type ResolveResult =
  | NpmResolveResult
  | JsrResolveResult
  | GitResolveResult
  | LocalResolveResult
  | UrlResolveResult
  | WorkspaceResolveResult

export interface ResolveResultBase {
  id: PkgResolutionId
  latest?: string
  publishedAt?: string
  manifest?: DependencyManifest
  resolution: Resolution
  resolvedVia: ResolvedVia
  normalizedBareSpecifier?: string
  alias?: string
}

export interface NpmResolveResult extends ResolveResultBase {
  latest: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'npm-registry'
}

export interface JsrResolveResult extends ResolveResultBase {
  alias: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'jsr-registry'
}

export interface GitResolveResult extends ResolveResultBase {
  resolution: GitResolution | TarballResolution
  resolvedVia: 'git-repository'
}

export interface LocalResolveResult extends ResolveResultBase {
  manifest?: DependencyManifest
  normalizedBareSpecifier: string
  resolution: DirectoryResolution | TarballResolution
  resolvedVia: 'local-filesystem'
}

export interface UrlResolveResult extends ResolveResultBase {
  normalizedBareSpecifier: string
  resolution: TarballResolution
  resolvedVia: 'url'
}

/**
 * A dependency on a workspace package.
 */
export interface WorkspaceResolveResult extends ResolveResultBase {
  /**
   * 'workspace' will be returned for workspace: protocol dependencies or a
   * package in the workspace that matches the wanted dependency's name and
   * version range.
   */
  resolvedVia: 'workspace'
}

export interface WorkspacePackage {
  rootDir: ProjectRootDir
  manifest: DependencyManifest
}

export type WorkspacePackagesByVersion = Map<string, WorkspacePackage>

export type WorkspacePackages = Map<string, WorkspacePackagesByVersion>

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
  workspacePackages?: WorkspacePackages
  update?: false | 'compatible' | 'latest'
  injectWorkspacePackages?: boolean
  calcSpecifier?: boolean
  pinnedVersion?: PinnedVersion
}

export type WantedDependency = {
  injected?: boolean
  prevSpecifier?: string
} & ({
  alias?: string
  bareSpecifier: string
} | {
  alias: string
  bareSpecifier?: string
})

export type ResolveFunction = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<ResolveResult>
