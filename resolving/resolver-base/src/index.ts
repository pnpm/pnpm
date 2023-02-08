import { DependencyManifest } from '@pnpm/types'

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: undefined
  tarball: string
  integrity?: string
  // needed in some cases to get the auth token
  // sometimes the tarball URL is under a different path
  // and the auth token is specified for the registry only
  registry?: string
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
  TarballResolution |
  DirectoryResolution |
  GitResolution |
  ({ type: string } & object)

export interface ResolveResult {
  id: string
  latest?: string
  publishedAt?: string
  manifest?: DependencyManifest
  normalizedPref?: string // is null for npm-hosted dependencies
  resolution: Resolution
  resolvedVia: 'npm-registry' | 'git-repository' | 'local-filesystem' | 'url' | string
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
