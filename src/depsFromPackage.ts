import {PackageJson, Dependencies} from '@pnpm/types'

export default function depsFromPackage (pkg: PackageJson): Dependencies {
  return Object.assign(
    {},
    pkg.devDependencies,
    pkg.dependencies,
    pkg.optionalDependencies
  ) as Dependencies
}
