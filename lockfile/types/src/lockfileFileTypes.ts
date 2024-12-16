import { type LockfileBase, type LockfilePackageInfo, type LockfilePackageSnapshot, type ProjectSnapshotBase } from '.'

export interface LockfileFile extends LockfileBase {
  importers?: Record<string, LockfileFileProjectSnapshot>
  packages?: Record<string, LockfilePackageInfo>
  snapshots?: Record<string, LockfilePackageSnapshot>
}

/**
 * Similar to the current ProjectSnapshot interface, but omits the "specifiers"
 * field in favor of inlining each specifier next to its version resolution in
 * dependency blocks.
 */
export interface LockfileFileProjectSnapshot extends ProjectSnapshotBase {
  dependencies?: LockfileFileProjectResolvedDependencies
  devDependencies?: LockfileFileProjectResolvedDependencies
  optionalDependencies?: LockfileFileProjectResolvedDependencies
}

export interface LockfileFileProjectResolvedDependencies {
  [depName: string]: SpecifierAndResolution
}

export interface SpecifierAndResolution {
  specifier: string
  version: string
}
