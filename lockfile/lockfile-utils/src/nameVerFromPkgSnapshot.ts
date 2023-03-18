import { type PackageSnapshot } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'

export function nameVerFromPkgSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot
) {
  if (!pkgSnapshot.name) {
    const pkgInfo = dp.parse(depPath)
    return {
      name: pkgInfo.name as string,
      peersSuffix: pkgInfo.peersSuffix,
      version: pkgInfo.version as string,
    }
  }
  return {
    name: pkgSnapshot.name,
    peersSuffix: undefined,
    version: pkgSnapshot.version as string,
  }
}
