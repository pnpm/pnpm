import { type PackageSnapshot } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'

export function packageIdFromSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot
) {
  if (pkgSnapshot.id) return pkgSnapshot.id
  return dp.tryGetPackageId(depPath) ?? depPath
}
