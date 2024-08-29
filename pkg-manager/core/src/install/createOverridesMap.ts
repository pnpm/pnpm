import { type ProjectSnapshot } from '@pnpm/lockfile.types'
import { type VersionOverride } from '@pnpm/parse-overrides'

/**
 * Create an `overrides` object with references updated from the lockfile.
 * The `overrides` object would then be used to check if the lockfile's `overrides` is outdated.
 */
export function createOverridesMap (parsedOverrides: VersionOverride[] | undefined, rootSnapshot: ProjectSnapshot | undefined): Record<string, string> {
  if (!parsedOverrides || parsedOverrides.length === 0) return {}
  const overridesMap: Record<string, string> = Object.fromEntries(parsedOverrides.map(({ selector, newPref }) => [selector, newPref]))
  if (!rootSnapshot) return overridesMap
  const allDeps: Record<string, string> = {
    ...rootSnapshot.devDependencies,
    ...rootSnapshot.dependencies,
    ...rootSnapshot.optionalDependencies,
  }
  for (const { selector, refTarget } of parsedOverrides) {
    if (!refTarget) continue
    const targetDep: string | undefined = allDeps[refTarget]
    if (targetDep) {
      overridesMap[selector] = targetDep
    }
  }
  return overridesMap
}
