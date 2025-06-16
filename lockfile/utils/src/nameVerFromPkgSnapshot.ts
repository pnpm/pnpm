import { type PackageSnapshot } from '@pnpm/lockfile.types'
import * as dp from '@pnpm/dependency-path'
import { type PkgResolutionId } from '@pnpm/types'

export interface NameVer {
  name: string
  peerDepsGraphHash: string | undefined
  version: string
  nonSemverVersion?: PkgResolutionId
}

export function nameVerFromPkgSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot
): NameVer {
  const pkgInfo = dp.parse(depPath)
  return {
    name: pkgInfo.name as string,
    peerDepsGraphHash: pkgInfo.peerDepsGraphHash,
    version: pkgSnapshot.version ?? pkgInfo.version as string,
    nonSemverVersion: pkgInfo.nonSemverVersion,
  }
}
