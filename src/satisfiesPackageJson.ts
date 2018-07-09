import {PackageJson} from '@pnpm/types'
import R = require('ramda')
import {
  Shrinkwrap,
} from './types'

export default (shr: Shrinkwrap, pkg: PackageJson) => {
  if (!R.equals({...pkg.devDependencies, ...pkg.dependencies, ...pkg.optionalDependencies}, shr.specifiers)) {
    return false
  }
  for (const depType of ['optionalDependencies', 'dependencies', 'devDependencies']) {
    const emptyDep = R.isEmpty(R.keys(pkg[depType]))
    if (emptyDep !== R.isEmpty(R.keys(shr[depType]))) return false
    if (emptyDep) continue

    const pkgDepNames = depType === 'optionalDependencies'
      ? Object.keys(pkg.optionalDependencies || {})
      : Object.keys(pkg[depType]).filter((depName) => !pkg.optionalDependencies || !pkg.optionalDependencies[depName])
    if (pkgDepNames.length !== Object.keys(shr[depType]).length &&
      pkgDepNames.length !== countOfNonLinkedDeps(shr[depType])) {
        return false
      }
    for (const depName of pkgDepNames) {
      if (!shr[depType][depName] || shr.specifiers[depName] !== pkg[depType][depName]) return false
    }
  }
  return true
}

function countOfNonLinkedDeps (shrDeps: {[depName: string]: string}): number {
  return R.values(shrDeps).filter((ref) => ref.indexOf('link:') === -1 && ref.indexOf('file:') === -1).length
}
