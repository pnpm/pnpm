export interface Shrinkwrap {
  shrinkwrapVersion: number,
  // Should be deprecated from shrinkwrap version 4
  shrinkwrapMinorVersion?: number,
  specifiers: ResolvedDependencies,
  dependencies?: ResolvedDependencies,
  optionalDependencies?: ResolvedDependencies,
  devDependencies?: ResolvedDependencies,
  packages?: ResolvedPackages,
  registry: string,
}

// For backward compatibility
export type ResolvedPackages = PackageSnapshots

export interface PackageSnapshots {
  [packagePath: string]: PackageSnapshot,
}

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

/**
 * Git repository
 */
export interface GitRepositoryResolution {
  type: 'git',
  repo: string,
  commit: string,
}

export type Resolution =
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution

export type ShrinkwrapResolution = Resolution | {
  integrity: string,
}

// For backward compatibility
export type DependencyShrinkwrap = PackageSnapshot

export interface PackageSnapshot {
  id?: string,
  dev?: true | false,
  optional?: true,
  requiresBuild?: true,
  prepare?: true,
  hasBin?: true,
  // name and version are only needed
  // for packages that are hosted not in the npm registry
  name?: string,
  version?: string,
  resolution: ShrinkwrapResolution,
  dependencies?: ResolvedDependencies,
  optionalDependencies?: ResolvedDependencies,
  peerDependencies?: {
    [name: string]: string,
  },
  bundledDependencies?: {
    [name: string]: string,
  },
  engines?: {
    node: string,
  },
  os?: string[],
  cpu?: string[],
  deprecated?: string,
}

export interface Dependencies {
  [name: string]: string
}

export type PackageBin = string | {[name: string]: string}

/*** @example
 * {
 *   "foo": "registry.npmjs.org/foo/1.0.1"
 * }
 */
export interface ResolvedDependencies {
  [pkgName: string]: string,
}
