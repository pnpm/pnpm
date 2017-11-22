import {PackageJson, Dependencies} from '@pnpm/types'

export default function depsFromPackage (pkg: PackageJson): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies
  } as Dependencies
}
