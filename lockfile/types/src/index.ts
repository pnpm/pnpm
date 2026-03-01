import { type PatchFile } from '@pnpm/patching.types'
import { type DependenciesMeta, type DepPath, type ProjectId } from '@pnpm/types'
import { type PlatformAssetTarget } from '@pnpm/resolver-base'

export type { PatchFile, ProjectId }

export * from './lockfileFileTypes.js'

export interface LockfileSettings {
  autoInstallPeers?: boolean
  excludeLinksFromLockfile?: boolean
  peersSuffixMaxLength?: number
  injectWorkspacePackages?: boolean
}

export interface LockfileBase {
  catalogs?: CatalogSnapshots
  ignoredOptionalDependencies?: string[]
  lockfileVersion: string
  overrides?: Record<string, string>
  packageExtensionsChecksum?: string
  patchedDependencies?: Record<string, PatchFile>
  pnpmfileChecksum?: string
  settings?: LockfileSettings
  time?: Record<string, string>
}

export interface LockfileObject extends LockfileBase {
  importers: Record<ProjectId, ProjectSnapshot>
  packages?: PackageSnapshots
}

export interface LockfilePackageSnapshot {
  optional?: true
  dependencies?: ResolvedDependencies
  optionalDependencies?: ResolvedDependencies
  transitivePeerDependencies?: string[]
}

export interface LockfilePackageInfo {
  id?: string
  patched?: true
  hasBin?: true
  // name and version are only needed
  // for packages that are hosted not in the npm registry
  name?: string
  version?: string
  resolution: LockfileResolution
  peerDependencies?: {
    [name: string]: string
  }
  peerDependenciesMeta?: {
    [name: string]: {
      optional: true
    }
  }
  bundledDependencies?: string[] | boolean
  engines?: Record<string, string> & {
    node: string
  }
  os?: string[]
  cpu?: string[]
  libc?: string[]
  deprecated?: string
}

export interface ProjectSnapshotBase {
  dependenciesMeta?: DependenciesMeta
  publishDirectory?: string
}

export interface ProjectSnapshot extends ProjectSnapshotBase {
  specifiers: ResolvedDependencies
  dependencies?: ResolvedDependencies
  optionalDependencies?: ResolvedDependencies
  devDependencies?: ResolvedDependencies
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

export interface BinaryResolution {
  type: 'binary'
  url: string
  integrity: string
  bin: string | Record<string, string>
  archive: 'zip' | 'tarball'
}

export interface PlatformAssetResolution {
  resolution: Resolution
  targets: PlatformAssetTarget[]
}

/**
 * Custom resolution type for custom resolver-provided packages.
 * The type field must be prefixed with 'custom:' to differentiate it from built-in resolution types.
 *
 * Example: { type: 'custom:cdn', cdnUrl: '...' }
 */
export interface CustomResolution {
  type: `custom:${string}` // e.g., 'custom:cdn', 'custom:artifactory'
  [key: string]: unknown
}

export type Resolution =
  TarballResolution |
  GitRepositoryResolution |
  DirectoryResolution |
  BinaryResolution |
  CustomResolution

export interface VariationsResolution {
  type: 'variations'
  variants: PlatformAssetResolution[]
}

export type LockfileResolution = Resolution | VariationsResolution | {
  integrity: string
}

export type PackageSnapshot = LockfilePackageInfo & LockfilePackageSnapshot

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
