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

export type Resolution =
  TarballResolution |
  DirectoryResolution |
  ({ type: string } & object)

export interface ResolveResult {
  id: string
  latest?: string
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

export interface VersionSelectors {
  [selector: string]: 'version' | 'range' | 'tag'
}

export interface PreferredVersions {
  [packageName: string]: VersionSelectors
}

export interface ResolveOptions {
  alwaysTryWorkspacePackages?: boolean
  defaultTag?: string
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean
  registry: string
  workspacePackages?: WorkspacePackages
}

export type WantedDependency = {
  alias?: string
  pref: string
} | {
  alias: string
  pref?: string
}

export type ResolveFunction = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<ResolveResult>
