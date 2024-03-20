import * as dp from '@pnpm/dependency-path'
import type { Registries, PackageSnapshot } from '@pnpm/types'

export function packageIdFromSnapshot(
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): string {
  if (pkgSnapshot.id) {
    return pkgSnapshot.id
  }

  return dp.tryGetPackageId(registries, depPath) ?? depPath
}
