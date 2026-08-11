import type { LockfileObject } from '@pnpm/lockfile.types'
import { resolvedPackageVersionsFromLockfile } from '@pnpm/lockfile.utils'

export interface MinimumReleaseAgeExcludePruneOptions {
  minimumReleaseAgeExcludePrune?: boolean
  lockfile?: boolean
  sharedWorkspaceLockfile?: boolean
}

/**
 * The versions `minimumReleaseAgeExcludePrune` prunes against, or
 * `undefined` when the pass must not run: it may only drop an entry it can
 * prove nothing resolves, so it needs a lockfile covering every project
 * `minimumReleaseAgeExclude` governs — only a workspace-shared one does.
 */
export function resolvedPackageVersionsForPrune (
  opts: MinimumReleaseAgeExcludePruneOptions,
  newLockfile: LockfileObject | undefined
): Map<string, Set<string>> | undefined {
  if (
    !opts.minimumReleaseAgeExcludePrune ||
    newLockfile == null ||
    opts.lockfile === false ||
    opts.sharedWorkspaceLockfile === false
  ) return undefined
  return resolvedPackageVersionsFromLockfile(newLockfile)
}
