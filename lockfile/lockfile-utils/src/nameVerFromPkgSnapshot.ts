import { type PackageSnapshot } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'

export interface NameVer {
  name: string
  peersSuffix: string | undefined
  version: string
}

export function nameVerFromPkgSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot
): NameVer {
  const pkgInfo = dp.parse(depPath)
  return {
    name: pkgInfo.name as string,
    peersSuffix: pkgInfo.peersSuffix,
    version: pkgSnapshot.version ?? pkgInfo.version as string,
  }
}
