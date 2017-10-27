export type Shrinkwrap = {
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

export type PackageSnapshots = {
  [packagePath: string]: PackageSnapshot,
}

/**
 * tarball hosted remotely
 */
export type TarballResolution = {
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
export type DirectoryResolution = {
  type: 'directory',
  directory: string,
}

/**
 * Git repository
 */
export type GitRepositoryResolution = {
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

export type PackageSnapshot = {
  id?: string,
  dev?: true,
  optional?: true,
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

export type Dependencies = {
  [name: string]: string
}

export type PackageBin = string | {[name: string]: string}

export type Package = {
  name: string,
  version: string,
  private?: boolean,
  bin?: PackageBin,
  directories?: {
    bin?: string,
  },
  dependencies?: Dependencies,
  devDependencies?: Dependencies,
  optionalDependencies?: Dependencies,
  peerDependencies?: Dependencies,
  bundleDependencies?: string[],
  bundledDependencies?: string[],
  scripts?: {
    [name: string]: string
  },
  config?: Object,
}

/*** @example
 * {
 *   "foo": "registry.npmjs.org/foo/1.0.1"
 * }
 */
export type ResolvedDependencies = {
  [pkgName: string]: string,
}
