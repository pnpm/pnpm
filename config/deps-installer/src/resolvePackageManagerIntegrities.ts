import { convertToLockfileFile, readConfigLockfile, writeConfigLockfile, createConfigLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { ConfigLockfile, PackageSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { StoreController } from '@pnpm/package-store'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'
import { resolveManifestDependencies } from './resolveManifestDependencies.js'

export interface ResolvePackageManagerIntegritiesOpts {
  registries: Registries
  rootDir: string
  storeController: StoreController
  storeDir: string
}

/**
 * Resolves integrity checksums for `pnpm`, `@pnpm/exe`, and their dependencies
 * by calling resolveManifestDependencies.
 * Writes the results to the `packageManagerDependencies` section of pnpm-config-lock.yaml.
 */
export async function resolvePackageManagerIntegrities (
  pnpmVersion: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<ConfigLockfile> {
  const configLockfile = (await readConfigLockfile(opts.rootDir)) ?? createConfigLockfile()

  // Check if already resolved for this version
  const pmDeps = configLockfile.importers['.'].packageManagerDependencies
  if (pmDeps != null) {
    const hasVersion = Object.values(pmDeps).some((dep) => dep.version === pnpmVersion)
    if (hasVersion) return configLockfile
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
    const lockfileFile = convertToLockfileFile(lockfile)
    const packageManagerDependencies: Record<string, { specifier: string, version: string }> = {}
    for (const pkgName of ['pnpm', '@pnpm/exe']) {
      packageManagerDependencies[pkgName] = {
        specifier: pnpmVersion,
        version: pnpmVersion,
      }
    }
    // Add new PM entries
    for (const [depPath, pkgInfo] of Object.entries(lockfileFile.packages ?? {})) {
      configLockfile.packages[depPath] = pkgInfo
    }
    for (const [depPath, snapshotInfo] of Object.entries(lockfileFile.snapshots ?? {})) {
      configLockfile.snapshots[depPath] = snapshotInfo
    }
    configLockfile.importers['.'].packageManagerDependencies = packageManagerDependencies
    // Prune orphan packages/snapshots using the lockfile pruner
    pruneConfigLockfile(configLockfile)
    await writeConfigLockfile(opts.rootDir, configLockfile)
  }
  return configLockfile
}

/**
 * Removes orphan packages/snapshots from the config lockfile by building
 * a LockfileObject and using pruneSharedLockfile to walk the dependency graph.
 */
function pruneConfigLockfile (configLockfile: ConfigLockfile): void {
  // Collect all dependency versions from importers as ResolvedDependencies
  const dependencies: ResolvedDependencies = {}
  for (const [name, dep] of Object.entries(configLockfile.importers['.'].configDependencies)) {
    dependencies[name] = dep.version
  }
  for (const [name, dep] of Object.entries(configLockfile.importers['.'].packageManagerDependencies ?? {})) {
    dependencies[name] = dep.version
  }

  // Merge packages and snapshots into the in-memory PackageSnapshots format
  const packages: Record<string, PackageSnapshot> = {}
  for (const [depPath, snapshot] of Object.entries(configLockfile.snapshots)) {
    packages[depPath as DepPath] = {
      ...snapshot,
      ...configLockfile.packages[depPath],
    }
  }

  // Use pruneSharedLockfile to walk the dep graph and keep only reachable entries
  const pruned = pruneSharedLockfile({
    lockfileVersion: configLockfile.lockfileVersion,
    importers: {
      ['.' as ProjectId]: {
        specifiers: {},
        dependencies,
      },
    },
    packages: packages as Record<DepPath, PackageSnapshot>,
  })

  // Split pruned packages back into separate packages and snapshots
  const prunedFile = convertToLockfileFile(pruned)
  configLockfile.packages = prunedFile.packages ?? {}
  configLockfile.snapshots = prunedFile.snapshots ?? {}
}
