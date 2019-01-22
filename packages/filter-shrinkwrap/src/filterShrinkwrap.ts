import { Shrinkwrap } from '@pnpm/shrinkwrap-types'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')
import filterImporter from './filterImporter'
import normalizeShrinkwrap from './normalizeShrinkwrap'

export default function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    defaultRegistry: string,
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped: Set<string>,
  },
): Shrinkwrap {
  let pairs = R.toPairs(shr.packages || {})
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
  return normalizeShrinkwrap({
    importers: Object.keys(shr.importers).reduce((acc, importerId) => {
      acc[importerId] = filterImporter(shr.importers[importerId], opts.include)
      return acc
    }, {}),
    packages: R.fromPairs(pairs),
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
  } as Shrinkwrap)
}
