import { convertToLockfileFile, convertToLockfileObject } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile } from '@pnpm/lockfile.types'

/**
 * Prunes stale packages and snapshots from an env lockfile by converting to
 * a standard lockfile object, pruning unreferenced entries, and converting back.
 */
export function pruneEnvLockfile (envLockfile: EnvLockfile): void {
  const merged = convertToLockfileObject({
    lockfileVersion: envLockfile.lockfileVersion,
    importers: {
      '.': {
        dependencies: {
          ...envLockfile.importers['.'].configDependencies,
          ...envLockfile.importers['.'].packageManagerDependencies,
        },
      },
    },
    packages: envLockfile.packages,
    snapshots: envLockfile.snapshots,
  })
  const pruned = pruneSharedLockfile(merged)
  const prunedFile = convertToLockfileFile(pruned)
  envLockfile.packages = prunedFile.packages ?? {}
  envLockfile.snapshots = prunedFile.snapshots ?? {}
}
