import { type ProjectRootDir, type DependencyManifest, type PkgResolutionId } from '@pnpm/types'

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

export interface ResolveResult {
  id: PkgResolutionId
  latest?: string
  publishedAt?: string
  manifest?: DependencyManifest
  normalizedPref?: string // is null for npm-hosted dependencies
  resolution: Resolution
  resolvedVia: 'npm-registry' | 'git-repository' | 'local-filesystem' | 'url' | string
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
  registry: string
  workspacePackages?: WorkspacePackages
  updateToLatest?: boolean
}

export type WantedDependency = {
  injected?: boolean
} & ({
  alias?: string
  pref: string
} | {
  alias: string
  pref?: string
})

export type ResolveFunction = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<ResolveResult>
