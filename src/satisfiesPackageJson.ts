import R = require('ramda')
import {
  Package,
  Shrinkwrap,
} from './types'

export default (shr: Shrinkwrap, pkg: Package) => {
  for (const depType of ['optionalDependencies', 'dependencies', 'devDependencies']) {
    const emptyDep = R.isEmpty(R.keys(pkg[depType]))
    if (emptyDep !== R.isEmpty(R.keys(shr[depType]))) return false
    if (emptyDep) continue

    if (depType === 'optionalDependencies') {
      const pkgODeps = pkg.optionalDependencies || {}
      const shrODeps = shr.optionalDependencies || {}
      for (const depName of Object.keys(pkgODeps)) {
        if (!shrODeps[depName] || shr.specifiers[depName] !== pkgODeps[depName]) return false
      }
    } else {
      for (const depName of Object.keys(pkg[depType])) {
        if (pkg.optionalDependencies && pkg.optionalDependencies[depName]) continue
        if (!shr[depType][depName] || shr.specifiers[depName] !== pkg[depType][depName]) return false
      }
    }
  }
  return true
}
