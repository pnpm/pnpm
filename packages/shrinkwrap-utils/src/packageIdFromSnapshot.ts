import { PackageSnapshot } from '@pnpm/shrinkwrap-types'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'

export default (
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries,
) => {
  if (pkgSnapshot.id) return pkgSnapshot.id
  if (relDepPath[0] !== '/') {
    return relDepPath
  }
  if (relDepPath[1] === '@') {
    return dp.resolve(registries, relDepPath.split('/').slice(0, 4).join(''))
  }
  return dp.resolve(registries, relDepPath.split('/').slice(0, 3).join(''))
}
