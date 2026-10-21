import { convertToLockfileFile, convertToLockfileObject } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'

/**
 * Separate importers preserve both pins when config and package-manager
 * dependencies use different versions of the same package.
 */
export function convertToLockfileEnvObject (envLockfile: EnvLockfile): LockfileObject {
  return convertToLockfileObject({
    lockfileVersion: envLockfile.lockfileVersion,
    importers: {
      '.': {
        dependencies: envLockfile.importers['.'].configDependencies,
      },
      packageManager: {
        dependencies: envLockfile.importers['.'].packageManagerDependencies ?? {},
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

/**
 * Retains declared config roots, preserving package-manager dependencies.
 * Returns whether a config root was removed.
 */
export function retainConfigDependencies (envLockfile: EnvLockfile, isDeclared: (name: string) => boolean): boolean {
  const dependencies = envLockfile.importers['.'].configDependencies
  let removed = false
  for (const name of Object.keys(dependencies)) {
    if (isDeclared(name)) continue
    delete dependencies[name]
    removed = true
  }
  return removed
}
