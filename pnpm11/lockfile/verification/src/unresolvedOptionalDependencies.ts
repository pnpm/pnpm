import { createMatcher } from '@pnpm/config.matcher'
import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import type { ProjectManifest } from '@pnpm/types'
import { pickBy } from 'ramda'

/**
 * The `optionalDependencies` of `pkg` that `importer` has no entry for.
 * The install that wrote the lockfile could not resolve them and skipped
 * them, so a frozen install skips them again instead of reporting them as
 * added. Configured `ignoredOptionalDependencies` and, under
 * `excludeLinksFromLockfile`, `link:` dependencies are absent by design and
 * are not returned.
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
