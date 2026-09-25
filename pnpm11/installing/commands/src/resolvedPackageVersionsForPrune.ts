import { readWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { resolvedPackageVersionsFromLockfile } from '@pnpm/lockfile.utils'

export interface MinimumReleaseAgeExcludePruneOptions {
  lockfile?: boolean
  sharedWorkspaceLockfile?: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

/**
 * The versions the freshly resolved lockfile records, or `undefined` when the
 * pass must not run.
 */
export function resolvedPackageVersionsForPrune (
  opts: MinimumReleaseAgeExcludePruneOptions,
  newLockfile: LockfileObject | undefined
): Map<string, Set<string>> | undefined {
  if (
    newLockfile == null ||
    opts.lockfile === false ||
    opts.sharedWorkspaceLockfile === false
  ) return undefined
  return resolvedPackageVersionsFromLockfile(newLockfile)
}

/**
 * The versions the lockfiles of a workspace with a lockfile per project record
 * together, read back once every workspace project is installed, or
 * `undefined` when the pass must not run. An entry is then pruned only when no
 * project's lockfile records it. A project without a lockfile has recorded
 * nothing to prove with, so it disables the pass.
 */
export async function resolvedPackageVersionsOfProjectLockfiles (
  opts: MinimumReleaseAgeExcludePruneOptions,
  projectDirs: string[]
): Promise<Map<string, Set<string>> | undefined> {
  if (opts.lockfile === false || projectDirs.length === 0) return undefined
  const resolved = new Map<string, Set<string>>()
  for (const projectDir of projectDirs) {
    // eslint-disable-next-line no-await-in-loop
    const lockfile = await readWantedLockfile(projectDir, {
      ignoreIncompatible: true,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
    if (lockfile == null) return undefined
    for (const [name, versions] of resolvedPackageVersionsFromLockfile(lockfile)) {
      const merged = resolved.get(name)
      if (merged == null) {
        resolved.set(name, versions)
      } else {
        for (const version of versions) merged.add(version)
      }
    }
  }
  return resolved
}
