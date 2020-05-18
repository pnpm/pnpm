import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import R = require('ramda')
import filterImporter from './filterImporter'

export default function filterLockfile (
  lockfile: Lockfile,
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped: Set<string>,
  }
): Lockfile {
  let pairs = R.toPairs(lockfile.packages || {})
    .filter(([relDepPath, pkg]) => !opts.skipped.has(relDepPath))
  if (!opts.include.dependencies) {
    pairs = pairs.filter(([relDepPath, pkg]) => pkg.dev !== false || pkg.optional)
  }
  if (!opts.include.devDependencies) {
    pairs = pairs.filter(([relDepPath, pkg]) => pkg.dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    pairs = pairs.filter(([relDepPath, pkg]) => !pkg.optional)
  }
  return {
    importers: Object.keys(lockfile.importers).reduce((acc, importerId) => {
      acc[importerId] = filterImporter(lockfile.importers[importerId], opts.include)
      return acc
    }, {}),
    lockfileVersion: lockfile.lockfileVersion,
    packages: R.fromPairs(pairs),
  }
}
