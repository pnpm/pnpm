import { type Lockfile } from '@pnpm/lockfile-types'
import { type DependenciesField, type DepPath } from '@pnpm/types'
import { filterLockfileByImporters } from './filterLockfileByImporters'

export function filterLockfile (
  lockfile: Lockfile,
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<DepPath>
  }
): Lockfile {
  return filterLockfileByImporters(lockfile, Object.keys(lockfile.importers), {
    ...opts,
    failOnMissingDependencies: false,
  })
}
