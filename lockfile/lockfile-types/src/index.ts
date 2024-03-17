import '@total-typescript/ts-reset'
import type { DependenciesMeta, PatchFile } from '@pnpm/types'

export type { PatchFile }

export interface LockfileSettings {
  autoInstallPeers?: boolean | undefined
  excludeLinksFromLockfile?: boolean | undefined
}

export interface Lockfile {
  importers: Record<string, ProjectSnapshot>
  lockfileVersion: number | string
  time?: Record<string, string> | undefined
  packages?: PackageSnapshots | undefined
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
  overrides?: Record<string, string> | undefined
  packageExtensionsChecksum?: string | undefined
  patchedDependencies?: Record<string, PatchFile> | undefined
  settings?: LockfileSettings | undefined
}

export interface ProjectSnapshot {
  specifiers: ResolvedDependencies
  dependencies?: ResolvedDependencies | undefined
  optionalDependencies?: ResolvedDependencies | undefined
  devDependencies?: ResolvedDependencies | undefined
  dependenciesMeta?: DependenciesMeta | undefined
  publishDirectory?: string | undefined
}

export interface LockfileV6 {
  importers: Record<string, ProjectSnapshotV6>
  lockfileVersion: number | string
  time?: Record<string, string> | undefined
  packages?: PackageSnapshots | undefined
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
  overrides?: Record<string, string> | undefined
  packageExtensionsChecksum?: string | undefined
  patchedDependencies?: Record<string, PatchFile> | undefined
  settings?: LockfileSettings | undefined
}

export interface ProjectSnapshotV6 {
  specifiers: ResolvedDependenciesOfImporters
  dependencies?: ResolvedDependenciesOfImporters | undefined
  optionalDependencies?: ResolvedDependenciesOfImporters | undefined
  devDependencies?: ResolvedDependenciesOfImporters | undefined
  dependenciesMeta?: DependenciesMeta | undefined
  publishDirectory?: string | undefined
}

export type ResolvedDependenciesOfImporters = Record<
  string,
  { version: string; specifier: string }
>

export interface PackageSnapshots {
  [packagePath: string]: PackageSnapshot
}

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: string | undefined
  tarball: string
  integrity?: string | undefined
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
  | TarballResolution
  | GitRepositoryResolution
  | DirectoryResolution

export type LockfileResolution =
  | Resolution
  | {
    integrity: string
  }

export interface PackageSnapshot {
  id?: string | undefined
  dev?: boolean | undefined
  optional?: boolean | undefined
  requiresBuild?: boolean | undefined
  patched?: boolean | undefined
  prepare?: boolean | undefined
  hasBin?: boolean | undefined
  // name and version are only needed
  // for packages that are hosted not in the npm registry
  name?: string | undefined
  version?: string | undefined
  resolution: LockfileResolution
  dependencies?: ResolvedDependencies | undefined
  optionalDependencies?: ResolvedDependencies | undefined
  peerDependencies?: {
    [name: string]: string
  } | undefined
  peerDependenciesMeta?: {
    [name: string]: {
      optional: true
    }
  } | undefined
  transitivePeerDependencies?: string[] | undefined
  bundledDependencies?: string[] | boolean | undefined
  engines?: (Record<string, string> & {
    node: string
  }) | undefined
  os?: string[] | undefined
  cpu?: string[] | undefined
  libc?: string[] | undefined
  deprecated?: string | undefined
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
