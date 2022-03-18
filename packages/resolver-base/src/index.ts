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

// The `lockfile` version selector is unique and perhaps unintuitive. When
// we're importing from lockfiles we need both the resolved version of a
// package _and_ the originally requested version range. That would normally
// necessitate a different data structure. But in order to avoid potential
// performance issues we jam them together like so:
//
// Given:
//   package.json              -> "eslint": "^1.0.0"
//   lockfile resolved version -> 1.2.3
//
// We'd get a preferredVersions object:
//   {
//     eslint: {
//       '^1.0.0@1.2.3': 'lockfile'
//     }
//   }
export interface VersionSelectors {
  [selector: string]: 'version' | 'range' | 'tag' | 'lockfile'
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
  injected?: boolean
} & ({
  alias?: string
  pref: string
} | {
  alias: string
  pref?: string
})

export type ResolveFunction = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<ResolveResult>
