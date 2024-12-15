import { type LockfileObject, type LockfilePackageInfo, type LockfilePackageSnapshot, type ProjectSnapshotBase } from '.'

export type LockfileFile = Omit<LockfileObject, 'importers' | 'packages'> & {
  importers?: Record<string, InlineSpecifiersProjectSnapshot>
  packages?: Record<string, LockfilePackageInfo>
  snapshots?: Record<string, LockfilePackageSnapshot>
}

/**
 * Similar to the current ProjectSnapshot interface, but omits the "specifiers"
 * field in favor of inlining each specifier next to its version resolution in
 * dependency blocks.
 */
export interface InlineSpecifiersProjectSnapshot extends ProjectSnapshotBase {
  dependencies?: InlineSpecifiersResolvedDependencies
  devDependencies?: InlineSpecifiersResolvedDependencies
  optionalDependencies?: InlineSpecifiersResolvedDependencies
}

export interface InlineSpecifiersResolvedDependencies {
  [depName: string]: SpecifierAndResolution
}

export interface SpecifierAndResolution {
  specifier: string
  version: string
}
