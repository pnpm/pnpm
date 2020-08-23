import { PackageSnapshot } from '@pnpm/lockfile-types'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'

export default (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
) => {
  if (pkgSnapshot.id) return pkgSnapshot.id
  return dp.tryGetPackageId(registries, depPath) ?? depPath
}
