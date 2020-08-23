import { OutdatedPackage } from '@pnpm/outdated'
import { SEMVER_CHANGE } from '@pnpm/semver-diff'

export type OutdatedWithVersionDiff = OutdatedPackage & { change: SEMVER_CHANGE | null, diff?: [string[], string[]] }

/**
 * Default comparators used as the argument to `ramda.sortWith()`.
 */
export const DEFAULT_COMPARATORS = [
  sortBySemverChange,
  (o1: OutdatedWithVersionDiff, o2: OutdatedWithVersionDiff) =>
    o1.packageName.localeCompare(o2.packageName),
  (o1: OutdatedWithVersionDiff, o2: OutdatedWithVersionDiff) =>
    (o1.current && o2.current) ? o1.current.localeCompare(o2.current) : 0,
]

export function sortBySemverChange (outdated1: OutdatedWithVersionDiff, outdated2: OutdatedWithVersionDiff) {
  return pkgPriority(outdated1) - pkgPriority(outdated2)
}

function pkgPriority (pkg: OutdatedWithVersionDiff) {
  switch (pkg.change) {
  case null: return 0
  case 'fix': return 1
  case 'feature': return 2
  case 'breaking': return 3
  default: return 4
  }
}
