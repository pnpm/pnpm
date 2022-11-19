import type { Lockfile } from '@pnpm/lockfile-types'
import type { DependenciesMeta } from '@pnpm/types'

export const INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX = '-inlineSpecifiers'

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
  importers: Record<string, InlineSpecifiersProjectSnapshot>
}

/**
 * Similar to the current ProjectSnapshot interface, but omits the "specifiers"
 * field in favor of inlining each specifier next to its version resolution in
 * dependency blocks.
 */
export interface InlineSpecifiersProjectSnapshot {
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
