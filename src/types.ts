export type Shrinkwrap = {
  version: number,
  specifiers: ResolvedDependencies,
  dependencies: ResolvedDependencies,
  optionalDependencies?: ResolvedDependencies,
  devDependencies?: ResolvedDependencies,
  packages: ResolvedPackages,
  registry: string,
}

export type ResolvedPackages = {
  [pkgId: string]: DependencyShrinkwrap,
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

export type DependencyShrinkwrap = {
  id?: string,
  dev?: true,
  optional?: true,
  resolution: ShrinkwrapResolution,
  dependencies?: ResolvedDependencies,
  optionalDependencies?: ResolvedDependencies,
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
