import { type OutdatedPackage } from '@pnpm/outdated'
import { type SEMVER_CHANGE } from '@pnpm/semver-diff'

export interface OutdatedWithVersionDiff extends OutdatedPackage {
  change: SEMVER_CHANGE | null
  diff?: [string[], string[]]
}

export type Comparator = (o1: OutdatedWithVersionDiff, o2: OutdatedWithVersionDiff) => number

export const NAME_COMPARATOR: Comparator = (o1, o2) => o1.packageName.localeCompare(o2.packageName)
/**
 * Default comparators used as the argument to `ramda.sortWith()`.
 */
export const DEFAULT_COMPARATORS: Comparator[] = [
  sortBySemverChange,
  NAME_COMPARATOR,
  (o1, o2) => (o1.current && o2.current) ? o1.current.localeCompare(o2.current) : 0, // eslint-disable-line @typescript-eslint/explicit-module-boundary-types
]

export function sortBySemverChange (outdated1: OutdatedWithVersionDiff, outdated2: OutdatedWithVersionDiff): number {
  return pkgPriority(outdated1) - pkgPriority(outdated2)
}

function pkgPriority (pkg: OutdatedWithVersionDiff): number {
  switch (pkg.change) {
  case null: return 0
  case 'fix': return 1
  case 'feature': return 2
  case 'breaking': return 3
  default: return 4
  }
}
