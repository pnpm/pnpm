import {
  DEPENDENCIES_FIELDS,
  PackageJson,
} from '@pnpm/types'
import R = require('ramda')
import {
  Shrinkwrap,
} from './types'

export default (shr: Shrinkwrap, pkg: PackageJson) => {
  if (!R.equals({...pkg.devDependencies, ...pkg.dependencies, ...pkg.optionalDependencies}, shr.specifiers)) {
    return false
  }
  for (const depField of DEPENDENCIES_FIELDS) {
    const shrDeps = shr[depField] || {}
    const pkgDeps = pkg[depField] || {}
    const emptyDep = R.isEmpty(pkgDeps)
    if (emptyDep !== R.isEmpty(shrDeps)) return false
    if (emptyDep) continue

    const pkgDepNames = depField === 'optionalDependencies'
      ? Object.keys(pkgDeps)
      : Object.keys(pkgDeps).filter((depName) => !pkg.optionalDependencies || !pkg.optionalDependencies[depName])
    if (pkgDepNames.length !== R.keys(shrDeps).length &&
      pkgDepNames.length !== countOfNonLinkedDeps(shrDeps)) {
        return false
      }
    for (const depName of pkgDepNames) {
      if (!shrDeps[depName] || shr.specifiers[depName] !== pkgDeps[depName]) return false
    }
  }
  return true
}

function countOfNonLinkedDeps (shrDeps: {[depName: string]: string}): number {
  return R.values(shrDeps).filter((ref) => ref.indexOf('link:') === -1 && ref.indexOf('file:') === -1).length
}
