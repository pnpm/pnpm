import { parsePkgAndParentSelector } from '@pnpm/config.parse-overrides'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

/**
 * The aliases targeted by a selector the lockfile records but `overrides` no
 * longer sets to the same value. A version such an override locked can still
 * satisfy the declared range, so these aliases are resolved as if the lockfile
 * held no version of them.
 */
export function getStaleOverrideTargets (
  lockedOverrides: Record<string, string> | undefined,
  overrides: Record<string, string>
): Set<string> {
  const targets = new Set<string>()
  for (const [selector, value] of Object.entries(lockedOverrides ?? {})) {
    if (overrides[selector] !== value) {
      targets.add(parsePkgAndParentSelector(selector).targetPkg.name)
    }
  }
  return targets
}

export function omitPackagesNamed (
  packages: LockfileObject['packages'],
  names: ReadonlySet<string>
): LockfileObject['packages'] {
  if (packages == null || names.size === 0) return packages
  return Object.fromEntries(
    Object.entries(packages).filter(([depPath, snapshot]) =>
      !names.has(nameVerFromPkgSnapshot(depPath as DepPath, snapshot).name))
  ) as LockfileObject['packages']
}
