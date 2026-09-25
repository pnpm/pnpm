import { createMatcher } from '@pnpm/config.matcher'
import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import type { ProjectManifest } from '@pnpm/types'
import { pickBy } from 'ramda'

/**
 * Returns a map of package names to requested specifiers for any direct
 * `optionalDependencies` of `pkg` that `importer.specifiers` does not contain.
 *
 * Called during frozen-lockfile verification to identify dependencies that
 * could not be resolved when the lockfile was written so they can be skipped
 * rather than reported as added.
 *
 * Returns an empty record if `pkg.optionalDependencies` is absent or empty.
 * Dependencies matching configured `ignoredOptionalDependencies` or starting
 * with `link:` when `excludeLinksFromLockfile` is enabled are omitted from
 * the result.
 */
export function unresolvedOptionalDependencies (
  opts: {
    excludeLinksFromLockfile?: boolean
    ignoredOptionalDependencies?: string[]
  },
  importer: Pick<ProjectSnapshot, 'specifiers'>,
  pkg: Pick<ProjectManifest, 'optionalDependencies'>
): Record<string, string> {
  const isIgnored = createMatcher(opts.ignoredOptionalDependencies ?? [])
  return pickBy(
    (bareSpecifier, depName) =>
      importer.specifiers[depName] == null &&
      !isIgnored(depName) &&
      !(opts.excludeLinksFromLockfile && bareSpecifier.startsWith('link:')),
    pkg.optionalDependencies ?? {}
  )
}
