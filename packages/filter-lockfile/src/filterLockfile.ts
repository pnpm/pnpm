import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import fromPairs from 'ramda/src/fromPairs'
import mapValues from 'ramda/src/map'
import { filterImporter } from './filterImporter'

export function filterLockfile (
  lockfile: Lockfile,
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<string>
  }
): Lockfile {
  let pairs = Object.entries(lockfile.packages ?? {})
    .filter(([depPath]) => !opts.skipped.has(depPath))
  if (!opts.include.dependencies) {
    pairs = pairs.filter(([_, pkg]) => pkg.dev !== false || pkg.optional)
  }
  if (!opts.include.devDependencies) {
    pairs = pairs.filter(([_, pkg]) => pkg.dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    pairs = pairs.filter(([_, pkg]) => !pkg.optional)
  }
  return {
    ...lockfile,
    importers: mapValues((importer) => filterImporter(importer, opts.include), lockfile.importers),
    packages: fromPairs(pairs),
  }
}
