import { type LockfileObject } from '@pnpm/lockfile.types'
import { type DependenciesField, type DepPath, type ProjectId } from '@pnpm/types'
import { filterLockfileByImporters } from './filterLockfileByImporters'

export function filterLockfile (
  lockfile: LockfileObject,
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<DepPath>
  }
): LockfileObject {
  return filterLockfileByImporters(lockfile, Object.keys(lockfile.importers) as ProjectId[], {
    ...opts,
    failOnMissingDependencies: false,
  })
}
