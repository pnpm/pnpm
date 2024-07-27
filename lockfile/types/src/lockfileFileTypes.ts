import type { Lockfile, PackageSnapshot, ProjectSnapshot } from '.'
import type { DependenciesMeta } from '@pnpm/types'

export type LockfileFile = Omit<InlineSpecifiersLockfile, 'importers'> &
Partial<InlineSpecifiersProjectSnapshot> &
Partial<Pick<InlineSpecifiersLockfile, 'importers'>>

export type LockfileFileV9 = Omit<InlineSpecifiersLockfile, 'importers' | 'packages'> &
Partial<InlineSpecifiersProjectSnapshot> &
Partial<Pick<InlineSpecifiersLockfile, 'importers'>> & {
  packages?: Record<string, Pick<PackageSnapshot, 'resolution' | 'engines' | 'cpu' | 'os' | 'hasBin' | 'name' | 'version' | 'bundledDependencies' | 'peerDependencies' | 'peerDependenciesMeta' | 'deprecated'>>
  snapshots?: Record<string, Pick<PackageSnapshot, 'dependencies' | 'optionalDependencies' | 'patched' | 'optional' | 'transitivePeerDependencies' | 'id'>>
}

/**
 * Similar to the current Lockfile importers format (lockfile version 5.4 at
 * time of writing), but specifiers are moved to each ResolvedDependencies block
 * instead of being declared on its own dictionary.
 *
 * This is an experiment to reduce one flavor of merge conflicts in lockfiles.
 * For more info: https://github.com/pnpm/pnpm/issues/4725.
 */
export interface InlineSpecifiersLockfile extends Omit<Lockfile, 'lockfileVersion' | 'importers'> {
  lockfileVersion: string
  importers?: Record<string, InlineSpecifiersProjectSnapshot>
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
