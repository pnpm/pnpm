import { PackageSnapshot } from '@pnpm/shrinkwrap-types'

export default ({ dependencies, optionalDependencies }: PackageSnapshot) => {
  return dependencies === undefined && optionalDependencies === undefined
}
