import type { PackageSnapshot } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'

export function nameVerFromPkgSnapshot(
  depPath: string,
  pkgSnapshot: PackageSnapshot | undefined
): {
    name: string;
    peersSuffix: string | undefined;
    version: string;
  } {
  if (!pkgSnapshot?.name) {
    const pkgInfo = dp.parse(depPath)
    return {
      name: pkgInfo.name ?? '',
      peersSuffix: pkgInfo.peersSuffix,
      version: pkgInfo.version ?? '',
    }
  }
  return {
    name: pkgSnapshot.name,
    peersSuffix: undefined,
    version: pkgSnapshot.version ?? '',
  }
}
