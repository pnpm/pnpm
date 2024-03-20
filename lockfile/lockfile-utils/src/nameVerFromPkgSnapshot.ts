import type { PackageSnapshot } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'

export function nameVerFromPkgSnapshot(
  depPath: string,
  pkgSnapshot: PackageSnapshot | undefined
): {
    name: string;
    peersSuffix: string | undefined;
    version: string;
  } {
  if (typeof pkgSnapshot?.name === 'undefined') {
    const pkgInfo = dp.parse(depPath)

    return {
      name: 'name' in pkgInfo ? pkgInfo.name : '',
      peersSuffix: 'peersSuffix' in pkgInfo ? pkgInfo.peersSuffix : undefined,
      version: 'version' in pkgInfo ? pkgInfo.version : '',
    }
  }

  return {
    name: pkgSnapshot.name ?? '',
    peersSuffix: undefined,
    version: pkgSnapshot.version ?? '',
  }
}
