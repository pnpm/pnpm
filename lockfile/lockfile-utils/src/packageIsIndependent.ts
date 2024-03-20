import type { PackageSnapshot } from '@pnpm/types'

export function packageIsIndependent({
  dependencies,
  optionalDependencies,
}: PackageSnapshot): boolean {
  return dependencies === undefined && optionalDependencies === undefined
}
