import { type DepPath } from '@pnpm/types'
import { type PackageSnapshot } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'

export function packageIdFromSnapshot (
  depPath: DepPath,
  pkgSnapshot: PackageSnapshot
): string {
  if (pkgSnapshot.id) return pkgSnapshot.id
  return dp.tryGetPackageId(depPath) ?? depPath
}
