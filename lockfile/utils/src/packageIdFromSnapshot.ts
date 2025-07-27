import { type DepPath, type PkgId } from '@pnpm/types'
import { type PackageSnapshot } from '@pnpm/lockfile.types'
import * as dp from '@pnpm/dependency-path'

export function packageIdFromSnapshot (
  depPath: DepPath,
  pkgSnapshot: PackageSnapshot
): PkgId {
  if (pkgSnapshot.id) return pkgSnapshot.id as PkgId
  if (depPath.startsWith('node@runtime:') || depPath.startsWith('deno@runtime:')) {
    return depPath as unknown as PkgId
  }
  return dp.tryGetPackageId(depPath) ?? depPath
}
