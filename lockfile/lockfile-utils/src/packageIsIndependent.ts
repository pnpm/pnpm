import { PackageSnapshot } from '@pnpm/lockfile-types'

export function packageIsIndependent ({ dependencies, optionalDependencies }: PackageSnapshot) {
  return dependencies === undefined && optionalDependencies === undefined
}
