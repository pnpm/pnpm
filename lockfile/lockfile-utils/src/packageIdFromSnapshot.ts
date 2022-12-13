import { PackageSnapshot } from '@pnpm/lockfile-types'
import { Registries } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'

export function packageIdFromSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
) {
  if (pkgSnapshot.id) return pkgSnapshot.id
  return dp.tryGetPackageId(registries, depPath) ?? depPath
}
