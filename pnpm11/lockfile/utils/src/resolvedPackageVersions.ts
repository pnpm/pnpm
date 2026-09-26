import type { LockfileObject } from '@pnpm/lockfile.types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

/**
 * Maps every package in the lockfile to its resolved versions. Packages
 * resolved from a non-semver source (git, tarball, ...) register only their
 * name: their presence can still be confirmed, but no exact version can.
 */
export function resolvedPackageVersionsFromLockfile (lockfile: LockfileObject): Map<string, Set<string>> {
  const resolvedVersions = new Map<string, Set<string>>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath, snapshot)
    let versions = resolvedVersions.get(name)
    if (versions == null) {
      versions = new Set()
      resolvedVersions.set(name, versions)
    }
    if (nonSemverVersion == null && version != null) {
      versions.add(version)
    }
  }
  return resolvedVersions
}
