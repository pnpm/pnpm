import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import * as R from 'ramda'
import filterImporter from './filterImporter'

export default function filterLockfile (
  lockfile: Lockfile,
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<string>
  }
): Lockfile {
  let pairs = Object.entries(lockfile.packages ?? {})
    .filter(([depPath, pkg]) => !opts.skipped.has(depPath))
  if (!opts.include.dependencies) {
    pairs = pairs.filter(([depPath, pkg]) => pkg.dev !== false || pkg.optional)
  }
  if (!opts.include.devDependencies) {
    pairs = pairs.filter(([depPath, pkg]) => pkg.dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    pairs = pairs.filter(([depPath, pkg]) => !pkg.optional)
  }
  return {
    ...lockfile,
    importers: Object.keys(lockfile.importers).reduce((acc, importerId) => {
      acc[importerId] = filterImporter(lockfile.importers[importerId], opts.include)
      return acc
    }, {}),
    packages: R.fromPairs(pairs),
  }
}
