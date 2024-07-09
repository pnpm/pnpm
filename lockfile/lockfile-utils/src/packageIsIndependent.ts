import { type PackageSnapshot } from '@pnpm/lockfile-types'

export function packageIsIndependent ({ dependencies, optionalDependencies }: PackageSnapshot): boolean {
  return dependencies === undefined && optionalDependencies === undefined
}
