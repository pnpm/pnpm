import { PackageSnapshot } from '@pnpm/shrinkwrap-types'
import * as dp from 'dependency-path'

export default (
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
) => {
  if (!pkgSnapshot.name) {
    const pkgInfo = dp.parse(relDepPath)
    return {
      name: pkgInfo.name as string,
      version: pkgInfo.version as string,
    }
  }
  return {
    name: pkgSnapshot.name,
    version: pkgSnapshot.version as string,
  }
}
