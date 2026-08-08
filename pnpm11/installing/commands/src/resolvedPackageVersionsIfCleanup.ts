import type { LockfileObject } from '@pnpm/lockfile.types'
import { resolvedPackageVersionsFromLockfile } from '@pnpm/lockfile.utils'

export function resolvedPackageVersionsIfCleanup (
  opts: { cleanupOutdatedMinimumReleaseAgeExcludes?: boolean },
  newLockfile: LockfileObject | undefined
): Map<string, Set<string>> | undefined {
  if (!opts.cleanupOutdatedMinimumReleaseAgeExcludes || newLockfile == null) return undefined
  return resolvedPackageVersionsFromLockfile(newLockfile)
}

export function mergeResolvedPackageVersions (acc: Map<string, Set<string>>, newLockfile: LockfileObject): void {
  for (const [name, versions] of resolvedPackageVersionsFromLockfile(newLockfile)) {
    let target = acc.get(name)
    if (target == null) {
      target = new Set()
      acc.set(name, target)
    }
    for (const version of versions) {
      target.add(version)
    }
  }
}
