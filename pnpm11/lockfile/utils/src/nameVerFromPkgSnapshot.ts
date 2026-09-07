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

/**
 * The package identity a dependency path stands for.
 *
 * The version slot of a dependency path only holds a semver for a registry
 * package; a git / tarball / `file:` one holds its resolution there instead,
 * and the version it resolved to is recorded on the `packages:` entry. That
 * entry therefore wins when there is one. It can be absent: a reference may name a dependency
 * path the lockfile has no entry for, and a registry package's path still carries its version.
 */
export function nameVerFromPkgSnapshot (
  depPath: string,
  pkgSnapshot: PackageSnapshot | undefined
): NameVer {
  const pkgInfo = dp.parse(depPath)
  return {
    name: pkgInfo.name as string,
    peerDepGraphHash: pkgInfo.peerDepGraphHash,
    version: pkgSnapshot?.version ?? pkgInfo.version as string ?? undefined,
    nonSemverVersion: pkgInfo.nonSemverVersion,
    registryName: pkgInfo.registryName,
  }
}
