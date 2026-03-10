import { convertToLockfileFile, convertToLockfileObject, readEnvLockfile, writeEnvLockfile, createEnvLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile } from '@pnpm/lockfile.types'
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
 * Writes the results to the `packageManagerDependencies` section of pnpm-lock.env.yaml.
 */
export async function resolvePackageManagerIntegrities (
  pnpmVersion: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<EnvLockfile> {
  const envLockfile = (await readEnvLockfile(opts.rootDir)) ?? createEnvLockfile()

  // Check if already resolved for this version
  const pmDeps = envLockfile.importers['.'].packageManagerDependencies
  if (pmDeps != null) {
    const hasVersion = Object.values(pmDeps).some((dep) => dep.version === pnpmVersion)
    if (hasVersion) return envLockfile
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

    // Convert env lockfile to LockfileObject, merge new packages, prune, and split back
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
    for (const [depPath, pkg] of Object.entries(lockfile.packages)) {
      merged.packages![depPath as DepPath] = pkg
    }
    const pruned = pruneSharedLockfile(merged)
    const prunedFile = convertToLockfileFile(pruned)
    envLockfile.packages = prunedFile.packages ?? {}
    envLockfile.snapshots = prunedFile.snapshots ?? {}

    await writeEnvLockfile(opts.rootDir, envLockfile)
  }
  return envLockfile
}
