import type { LockfileObject, PackageSnapshot, ProjectSnapshot } from '.'
import type { DependenciesMeta } from '@pnpm/types'

export type LockfileFile = Omit<LockfileObject, 'importers' | 'packages'> & {
  importers?: Record<string, InlineSpecifiersProjectSnapshot>
  packages?: Record<string, Pick<PackageSnapshot, 'resolution' | 'engines' | 'cpu' | 'os' | 'hasBin' | 'name' | 'version' | 'bundledDependencies' | 'peerDependencies' | 'peerDependenciesMeta' | 'deprecated'>>
  snapshots?: Record<string, Pick<PackageSnapshot, 'dependencies' | 'optionalDependencies' | 'patched' | 'optional' | 'transitivePeerDependencies' | 'id'>>
}

/**
 * Similar to the current ProjectSnapshot interface, but omits the "specifiers"
 * field in favor of inlining each specifier next to its version resolution in
 * dependency blocks.
 */
export type InlineSpecifiersProjectSnapshot = Omit<ProjectSnapshot, 'dependencies' | 'devDependencies' | 'optionalDependencies' | 'dependenciesMeta' | 'specifiers'> & {
  dependencies?: InlineSpecifiersResolvedDependencies
  devDependencies?: InlineSpecifiersResolvedDependencies
  optionalDependencies?: InlineSpecifiersResolvedDependencies
  dependenciesMeta?: DependenciesMeta
}

export interface InlineSpecifiersResolvedDependencies {
  [depName: string]: SpecifierAndResolution
}

export interface SpecifierAndResolution {
  specifier: string
  version: string
}
