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
    for (const depName of Object.keys(pkg[depType])) {
      if (!shr[depType][depName] || shr.specifiers[depName] !== pkg[depType][depName]) return false
    }
  }
  return true
}
