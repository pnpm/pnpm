import { type DependenciesMeta, type PatchFile } from '@pnpm/types'

export type { PatchFile }

export * from './lockfileFileTypes'

export interface LockfileSettings {
  autoInstallPeers?: boolean
  excludeLinksFromLockfile?: boolean
}

export interface Lockfile {
  importers: Record<string, ProjectSnapshot>
  lockfileVersion: number | string
  time?: Record<string, string>
  packages?: PackageSnapshots
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  overrides?: Record<string, string>
  packageExtensionsChecksum?: string
  patchedDependencies?: Record<string, PatchFile>
  settings?: LockfileSettings
}

export interface ProjectSnapshot {
  specifiers: ResolvedDependencies
  dependencies?: ResolvedDependencies
  optionalDependencies?: ResolvedDependencies
  devDependencies?: ResolvedDependencies
  dependenciesMeta?: DependenciesMeta
  publishDirectory?: string
}

export type ResolvedDependenciesOfImporters = Record<string, { version: string, specifier: string }>

export interface PackageSnapshots {
  [packagePath: string]: PackageSnapshot
}

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: undefined
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

/**
 * Git repository
 */
export interface GitRepositoryResolution {
  type: 'git'
  repo: string
  commit: string
}

export type Resolution =
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution

export type LockfileResolution = Resolution | {
  integrity: string
}

export interface PackageSnapshot {
  id?: string
  dev?: true | false
  optional?: true
  requiresBuild?: true
  patched?: true
  prepare?: true
  hasBin?: true
  // name and version are only needed
  // for packages that are hosted not in the npm registry
  name?: string
  version?: string
  resolution: LockfileResolution
  dependencies?: ResolvedDependencies
  optionalDependencies?: ResolvedDependencies
  peerDependencies?: {
    [name: string]: string
  }
  peerDependenciesMeta?: {
    [name: string]: {
      optional: true
    }
  }
  transitivePeerDependencies?: string[]
  bundledDependencies?: string[] | boolean
  engines?: Record<string, string> & {
    node: string
  }
  os?: string[]
  cpu?: string[]
  libc?: string[]
  deprecated?: string
}

export interface Dependencies {
  [name: string]: string
}

export type PackageBin = string | { [name: string]: string }

/** @example
 * {
 *   "foo": "registry.npmjs.org/foo/1.0.1"
 * }
 */
export type ResolvedDependencies = Record<string, string>
