import { type DependenciesMeta, type DepPath, type PatchFile, type ProjectId } from '@pnpm/types'

export type { PatchFile, ProjectId }

export * from './lockfileFileTypes'

export interface LockfileSettings {
  autoInstallPeers?: boolean
  excludeLinksFromLockfile?: boolean
  peersSuffixMaxLength?: number
}

export interface Lockfile {
  importers: Record<ProjectId, ProjectSnapshot>
  lockfileVersion: string
  time?: Record<string, string>
  catalogs?: CatalogSnapshots
  packages?: PackageSnapshots
  overrides?: Record<string, string>
  packageExtensionsChecksum?: string
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: Record<string, PatchFile>
  pnpmfileChecksum?: string
  settings?: LockfileSettings
}

export interface LockfileV9 {
  importers: Record<string, ProjectSnapshot>
  lockfileVersion: string
  time?: Record<string, string>
  snapshots?: Record<string, PackageSnapshotV7>
  packages?: Record<string, PackageInfo>
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
  [packagePath: DepPath]: PackageSnapshot
}

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

/**
 * Git repository
 */
export interface GitRepositoryResolution {
  type: 'git'
  repo: string
  commit: string
  path?: string
}

export type Resolution =
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution

export type LockfileResolution = Resolution | {
  integrity: string
}

export type PackageSnapshotV7 = Pick<PackageSnapshot, 'optional' | 'dependencies' | 'optionalDependencies' | 'transitivePeerDependencies'>

export type PackageInfo = Pick<PackageSnapshot, 'id' | 'patched' | 'hasBin' | 'name' | 'version' | 'resolution' | 'peerDependencies' | 'peerDependenciesMeta' | 'bundledDependencies' | 'engines' | 'cpu' | 'os' | 'libc' | 'deprecated'>

export interface PackageSnapshot {
  id?: string
  optional?: true
  patched?: true
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

export interface CatalogSnapshots {
  [catalogName: string]: { [dependencyName: string]: ResolvedCatalogEntry }
}

export interface ResolvedCatalogEntry {
  /**
   * The real specifier that should be used for this dependency's catalog entry.
   * This would be the ^1.2.3 portion of:
   *
   * @example
   * catalog:
   *   foo: ^1.2.3
   */
  readonly specifier: string

  /**
   * The concrete version that the requested specifier resolved to. Ex: 1.2.3
   */
  readonly version: string
}
