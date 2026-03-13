import * as dp from '@pnpm/dependency-path'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import type { DepPath, PkgId } from '@pnpm/types'

export function packageIdFromSnapshot (
  depPath: DepPath,
  pkgSnapshot: PackageSnapshot
): PkgId {
  if (pkgSnapshot.id) return pkgSnapshot.id as PkgId
  return dp.tryGetPackageId(depPath) ?? depPath
}
