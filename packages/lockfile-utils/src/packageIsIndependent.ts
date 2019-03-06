import { PackageSnapshot } from '@pnpm/lockfile-types'

export default ({ dependencies, optionalDependencies }: PackageSnapshot) => {
  return dependencies === undefined && optionalDependencies === undefined
}
