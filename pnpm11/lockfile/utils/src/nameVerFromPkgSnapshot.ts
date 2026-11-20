import * as dp from '@pnpm/deps.path'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import type { PkgResolutionId } from '@pnpm/types'

export interface NameVer {
  name: string
  peerDepGraphHash: string | undefined
  version: string
  nonSemverVersion?: PkgResolutionId
  /** The named-registry alias of a registry-qualified dep path (`<name>@<registryName>:<version>`). */
  registryName?: string
}

export function nameVerFromPkgSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot
): NameVer {
  const pkgInfo = dp.parse(depPath)
  return {
    name: pkgInfo.name as string,
    peerDepGraphHash: pkgInfo.peerDepGraphHash,
    version: pkgSnapshot.version ?? pkgInfo.version as string ?? undefined,
    nonSemverVersion: pkgInfo.nonSemverVersion,
    registryName: pkgInfo.registryName,
  }
}
