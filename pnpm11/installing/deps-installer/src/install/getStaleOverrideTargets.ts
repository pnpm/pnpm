import { parsePkgAndParentSelector } from '@pnpm/config.parse-overrides'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

/**
 * The aliases targeted by an unscoped selector the lockfile records but
 * `overrides` no longer has. Such an override governed every edge of its
 * alias, and a version it locked can still satisfy the declared range, so
 * these aliases are resolved as if the lockfile held no version of them.
 *
 * A selector scoped to a parent or a version range governed only some edges
 * of its alias. Reopening every edge would move the others too, so a scoped
 * selector contributes nothing. A selector that is still set keeps governing
 * its edges, so a version it locked stays while the new value accepts it.
 */
export function getStaleOverrideTargets (
  lockedOverrides: Record<string, string> | undefined,
  overrides: Record<string, string>
): Set<string> {
  const targets = new Set<string>()
  for (const selector of Object.keys(lockedOverrides ?? {})) {
    if (overrides[selector] != null) continue
    const { parentPkg, targetPkg } = parsePkgAndParentSelector(selector)
    if (parentPkg == null && targetPkg.bareSpecifier == null) {
      targets.add(targetPkg.name)
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
