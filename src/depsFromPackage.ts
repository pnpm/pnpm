import {Package, Dependencies} from './types'

export default function depsFromPackage (pkg: Package): Dependencies {
  return Object.assign(
    {},
    pkg.devDependencies,
    pkg.dependencies,
    pkg.optionalDependencies
  ) as Dependencies
}
