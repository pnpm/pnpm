import { PackageSnapshot } from '@pnpm/shrinkwrap-types'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'

export default (
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries,
) => {
  if (pkgSnapshot.id) return pkgSnapshot.id
  return dp.tryGetPackageId(registries, relDepPath) || relDepPath
}
