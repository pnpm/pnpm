import { convertToLockfileFile, createEnvLockfile, readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import type { StoreController } from '@pnpm/store.controller'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'

import { convertToLockfileEnvObject } from './pruneEnvLockfile.js'
import { resolveManifestDependencies } from './resolveManifestDependencies.js'

export interface ResolvePackageManagerIntegritiesOpts {
  envLockfile?: EnvLockfile
  registries: Registries
  rootDir: string
  storeController: StoreController
  storeDir: string
  /**
   * Whether to read from and write to the env lockfile file on disk.
   * When false, resolution happens purely in memory; callers can still use
   * the returned `EnvLockfile` to perform installs without persisting the
   * resolved pnpm integrity info. Defaults to true.
   */
  save?: boolean
}

/**
 * Checks if the wanted pnpm version integrities are already fully resolved in the env lockfile.
 */
export function isPackageManagerResolved (
  envLockfile: EnvLockfile | undefined,
  pnpmVersion: string
): boolean {
  if (!envLockfile) return false

  const pmDeps = envLockfile.importers['.'].packageManagerDependencies
  return pmDeps != null &&
    pmDeps['pnpm']?.version === pnpmVersion &&
    pmDeps['@pnpm/exe']?.version === pnpmVersion
}

/**
 * Resolves integrity checksums for `pnpm`, `@pnpm/exe`, and their dependencies
 * by calling resolveManifestDependencies. When `opts.save` is true (the
 * default) the results are written to the `packageManagerDependencies`
 * section of `pnpm-lock.yaml`; when false, resolution happens purely in
 * memory and the returned `EnvLockfile` is never persisted to disk.
 */
export async function resolvePackageManagerIntegrities (
  pnpmVersion: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<EnvLockfile> {
  const save = opts.save ?? true
  const envLockfile = opts.envLockfile ?? (save ? await readEnvLockfile(opts.rootDir) : undefined) ?? createEnvLockfile()

  if (isPackageManagerResolved(envLockfile, pnpmVersion)) {
    return envLockfile
  }

  const lockfile = await resolveManifestDependencies(
    {
      dependencies: {
        'pnpm': pnpmVersion,
        '@pnpm/exe': pnpmVersion,
      },
    },
    {
      dir: opts.rootDir,
      registries: opts.registries,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
    }
  )

  if (lockfile.packages) {
    // Build packageManagerDependencies from the resolved lockfile importers
    const importer = lockfile.importers['.' as ProjectId]
    const packageManagerDependencies: Record<string, { specifier: string, version: string }> = {}
    for (const [name, version] of Object.entries(importer.dependencies ?? {})) {
      packageManagerDependencies[name] = {
        specifier: importer.specifiers[name],
        version,
      }
    }
    envLockfile.importers['.'].packageManagerDependencies = packageManagerDependencies

    // Merge new packages into the env lockfile object, then prune stale entries
    const merged = convertToLockfileEnvObject(envLockfile)
    for (const [depPath, pkg] of Object.entries(lockfile.packages)) {
      merged.packages![depPath as DepPath] = pkg
    }
    const pruned = pruneSharedLockfile(merged)
    const prunedFile = convertToLockfileFile(pruned)
    envLockfile.packages = prunedFile.packages ?? {}
    envLockfile.snapshots = prunedFile.snapshots ?? {}

    if (save) {
      await writeEnvLockfile(opts.rootDir, envLockfile)
    }
  }
  return envLockfile
}
