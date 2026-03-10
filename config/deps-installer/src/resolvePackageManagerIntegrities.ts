import { convertToLockfileFile, convertToLockfileObject, readConfigLockfile, writeConfigLockfile, createConfigLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { ConfigLockfile } from '@pnpm/lockfile.types'
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
    // Build packageManagerDependencies from the resolved lockfile importers
    const importer = lockfile.importers['.' as ProjectId]
    const packageManagerDependencies: Record<string, { specifier: string, version: string }> = {}
    for (const [name, version] of Object.entries(importer.dependencies ?? {})) {
      packageManagerDependencies[name] = {
        specifier: importer.specifiers[name],
        version,
      }
    }
    configLockfile.importers['.'].packageManagerDependencies = packageManagerDependencies

    // Convert config lockfile to LockfileObject, merge new packages, prune, and split back
    const merged = convertToLockfileObject({
      lockfileVersion: configLockfile.lockfileVersion,
      importers: {
        '.': {
          dependencies: {
            ...configLockfile.importers['.'].configDependencies,
            ...configLockfile.importers['.'].packageManagerDependencies,
          },
        },
      },
      packages: configLockfile.packages,
      snapshots: configLockfile.snapshots,
    })
    for (const [depPath, pkg] of Object.entries(lockfile.packages)) {
      merged.packages![depPath as DepPath] = pkg
    }
    const pruned = pruneSharedLockfile(merged)
    const prunedFile = convertToLockfileFile(pruned)
    configLockfile.packages = prunedFile.packages ?? {}
    configLockfile.snapshots = prunedFile.snapshots ?? {}

    await writeConfigLockfile(opts.rootDir, configLockfile)
  }
  return configLockfile
}
