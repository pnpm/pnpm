import { createMatcher } from '@pnpm/config.matcher'
import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { ProjectManifest } from '@pnpm/types'
import { pickBy } from 'ramda'

import { isExcludedLink } from './dependencySpec.js'

/**
 * Returns a map of package names to requested specifiers for any direct
 * `optionalDependencies` of `pkg` that `importer.specifiers` does not contain.
 *
 * Called during frozen-lockfile verification to identify dependencies that
 * could not be resolved when the lockfile was written so they can be skipped
 * rather than reported as added.
 *
 * Dependencies matching configured `ignoredOptionalDependencies`, and the
 * links `excludeLinksFromLockfile` leaves out, are omitted from the result.
 */
export function unresolvedOptionalDependencies (
  opts: {
    excludeLinksFromLockfile?: boolean
    ignoredOptionalDependencies?: string[]
    workspacePackages?: WorkspacePackages
  },
  importer: Pick<ProjectSnapshot, 'specifiers'>,
  pkg: Pick<ProjectManifest, 'optionalDependencies'>
): Record<string, string> {
  const isIgnored = createMatcher(opts.ignoredOptionalDependencies ?? [])
  return pickBy(
    (bareSpecifier, depName) =>
      importer.specifiers[depName] == null &&
      !isIgnored(depName) &&
      !(opts.excludeLinksFromLockfile && isExcludedLink(opts.workspacePackages, { importer, depName, spec: bareSpecifier })),
    pkg.optionalDependencies ?? {}
  )
}
