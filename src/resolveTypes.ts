import {PackageJson} from '@pnpm/types'

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: undefined,
  tarball: string,
  integrity?: string,
  // needed in some cases to get the auth token
  // sometimes the tarball URL is under a different path
  // and the auth token is specified for the registry only
  registry?: string,
}

/**
 * directory on a file system
 */
export interface DirectoryResolution {
  type: 'directory',
  directory: string,
}

export type Resolution =
  TarballResolution |
  DirectoryResolution |
  ({ type: string } & object)

export interface ResolveResult {
  id: string,
  resolution: Resolution,
  package?: PackageJson,
  latest?: string,
  normalizedPref?: string, // is null for npm-hosted dependencies
}

export interface ResolveOptions {
  defaultTag?: string,
  registry: string,
  prefix: string,
  preferredVersions: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
}

export type WantedDependency = {
  alias?: string,
  pref?: string,
} & (
  {alias: string, pref: string}
  | {alias: string}
  | {pref: string}
)

export type ResolveFunction = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<ResolveResult>
