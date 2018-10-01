import { Dependencies, PackageJson } from '@pnpm/types'

export default function getAllDependenciesFromPackage (pkg: PackageJson): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
  } as Dependencies
}
