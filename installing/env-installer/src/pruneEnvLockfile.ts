import { convertToLockfileFile, convertToLockfileObject } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'

/**
 * Converts an env lockfile to a standard LockfileObject by merging
 * configDependencies and packageManagerDependencies into a single
 * importers['.'].dependencies map.
 */
export function convertToLockfileEnvObject (envLockfile: EnvLockfile): LockfileObject {
  return convertToLockfileObject({
    lockfileVersion: envLockfile.lockfileVersion,
    importers: {
      '.': {
        dependencies: {
          ...envLockfile.importers['.'].configDependencies,
          ...(envLockfile.importers['.'].packageManagerDependencies ?? {}),
        },
      },
    },
    packages: envLockfile.packages,
    snapshots: envLockfile.snapshots,
  })
}

/**
 * Prunes stale packages and snapshots from an env lockfile by converting to
 * a standard lockfile object, pruning unreferenced entries, and converting back.
 */
export function pruneEnvLockfile (envLockfile: EnvLockfile): void {
  const lockfileObject = convertToLockfileEnvObject(envLockfile)
  const pruned = pruneSharedLockfile(lockfileObject)
  const prunedFile = convertToLockfileFile(pruned)
  envLockfile.packages = prunedFile.packages ?? {}
  envLockfile.snapshots = prunedFile.snapshots ?? {}
}
