import { writeSettings } from '@pnpm/config.config-writer'
import { PnpmError } from '@pnpm/error'
import { createEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import type { ConfigDependencies, ConfigDependencySpecifiers, Registries } from '@pnpm/types'
import getNpmTarballUrl from 'get-npm-tarball-url'

import type { NormalizedConfigDep } from './parseIntegrity.js'
import { parseIntegrity } from './parseIntegrity.js'

interface MigrateOpts {
  registries: Registries
  rootDir: string
}

/**
 * Migrates old-format configDependencies (with inline integrity in pnpm-workspace.yaml)
 * to the new pnpm-lock.yaml format.
 *
 * Returns normalized deps for immediate installation, and writes the env lockfile
 * and clean specifiers to pnpm-workspace.yaml as a side effect.
 */
export async function migrateConfigDepsToLockfile (
  configDeps: ConfigDependencies,
  opts: MigrateOpts
): Promise<Record<string, NormalizedConfigDep>> {
  const envLockfile = createEnvLockfile()
  const cleanSpecifiers: ConfigDependencySpecifiers = {}
  const normalizedDeps: Record<string, NormalizedConfigDep> = {}

  for (const [pkgName, pkgSpec] of Object.entries(configDeps)) {
    const registry = pickRegistryForPackage(opts.registries, pkgName)

    if (typeof pkgSpec === 'object') {
      const { version, integrity } = parseIntegrity(pkgName, pkgSpec.integrity)
      const tarball = pkgSpec.tarball ?? getNpmTarballUrl(pkgName, version, { registry })

      cleanSpecifiers[pkgName] = version
      const pkgKey = `${pkgName}@${version}`
      envLockfile.importers['.'].configDependencies[pkgName] = {
        specifier: version,
        version,
      }
      envLockfile.packages[pkgKey] = {
        resolution: toLockfileResolution(
          { name: pkgName, version },
          { integrity, tarball },
          registry
        ),
      }
      envLockfile.snapshots[pkgKey] = {}
      normalizedDeps[pkgName] = {
        version,
        resolution: { integrity, tarball },
      }
      continue
    }

    if (typeof pkgSpec === 'string') {
      // This branch only handles the legacy inline format (version+integrity).
      // New clean specifiers (just version/range) require an existing pnpm-lock.yaml.
      if (!pkgSpec.includes('+')) {
        throw new PnpmError(
          'CONFIG_DEP_MISSING_LOCKFILE',
          `Config dependency "${pkgName}" is already in clean-specifier form (${pkgSpec}) ` +
          'but no pnpm-lock.yaml was found to resolve it. ' +
          'Please generate and commit pnpm-lock.yaml (for example by running ' +
          '`pnpm install` in the workspace root) before attempting to migrate configDependencies.'
        )
      }
      const { version, integrity } = parseIntegrity(pkgName, pkgSpec)
      const tarball = getNpmTarballUrl(pkgName, version, { registry })

      cleanSpecifiers[pkgName] = version
      const pkgKey = `${pkgName}@${version}`
      envLockfile.importers['.'].configDependencies[pkgName] = {
        specifier: version,
        version,
      }
      envLockfile.packages[pkgKey] = {
        resolution: { integrity },
      }
      envLockfile.snapshots[pkgKey] = {}
      normalizedDeps[pkgName] = {
        version,
        resolution: { integrity, tarball },
      }
    }
  }

  // Write the new env lockfile and clean up workspace manifest
  await Promise.all([
    writeEnvLockfile(opts.rootDir, envLockfile),
    writeSettings({
      rootProjectManifestDir: opts.rootDir,
      workspaceDir: opts.rootDir,
      updatedSettings: {
        configDependencies: cleanSpecifiers,
      },
    }),
  ])

  return normalizedDeps
}
