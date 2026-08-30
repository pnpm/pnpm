import type { LockfileObject } from '@pnpm/lockfile.types'
import { resolvedPackageVersionsFromLockfile } from '@pnpm/lockfile.utils'

export interface MinimumReleaseAgeExcludePruneOptions {
  lockfile?: boolean
  sharedWorkspaceLockfile?: boolean
}

/**
 * The versions the freshly resolved lockfile records, or `undefined` when the
 * pass must not run.
 */
export function resolvedPackageVersionsForPrune (
  opts: MinimumReleaseAgeExcludePruneOptions,
  newLockfile: LockfileObject | undefined
): Map<string, Set<string>> | undefined {
  if (
    newLockfile == null ||
    opts.lockfile === false ||
    opts.sharedWorkspaceLockfile === false
  ) return undefined
  return resolvedPackageVersionsFromLockfile(newLockfile)
}
