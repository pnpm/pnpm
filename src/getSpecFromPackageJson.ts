import {PackageJson} from '@pnpm/types'

export default (pkg: PackageJson, depName: string) => {
  return pkg.dependencies && pkg.dependencies[depName]
    || pkg.devDependencies && pkg.devDependencies[depName]
    || pkg.optionalDependencies && pkg.optionalDependencies[depName]
}
