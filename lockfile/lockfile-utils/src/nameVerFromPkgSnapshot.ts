import { type PackageSnapshot } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'

export function nameVerFromPkgSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot
) {
  const pkgInfo = dp.parse(depPath)
  return {
    name: pkgInfo.name as string,
    peersSuffix: pkgInfo.peersSuffix,
    version: pkgSnapshot.version ?? pkgInfo.version as string,
  }
}
