import { PnpmError } from '@pnpm/error'
import { writeSettings } from '@pnpm/config.config-writer'
import { createConfigLockfile, writeConfigLockfile } from '@pnpm/lockfile.fs'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import type { ConfigDependencies, ConfigDependencySpecifiers, Registries } from '@pnpm/types'
import getNpmTarballUrl from 'get-npm-tarball-url'

interface MigrateOpts {
  registries: Registries
  rootDir: string
}

interface NormalizedConfigDep {
  version: string
  resolution: {
    integrity: string
    tarball: string
  }
}

/**
 * Migrates old-format configDependencies (with inline integrity in pnpm-workspace.yaml)
 * to the new pnpm-config-lock.yaml format.
 *
 * Returns normalized deps for immediate installation, and writes the config lockfile
 * and clean specifiers to pnpm-workspace.yaml as a side effect.
 */
export async function migrateConfigDepsToLockfile (
  configDeps: ConfigDependencies,
  opts: MigrateOpts
): Promise<Record<string, NormalizedConfigDep>> {
  const configLockfile = createConfigLockfile()
  const cleanSpecifiers: ConfigDependencySpecifiers = {}
  const normalizedDeps: Record<string, NormalizedConfigDep> = {}

  for (const [pkgName, pkgSpec] of Object.entries(configDeps)) {
    const registry = pickRegistryForPackage(opts.registries, pkgName)

    if (typeof pkgSpec === 'object') {
      const { version, integrity } = parseIntegrity(pkgName, pkgSpec.integrity)
      const tarball = pkgSpec.tarball ?? getNpmTarballUrl(pkgName, version, { registry })

      cleanSpecifiers[pkgName] = version
      const pkgKey = `${pkgName}@${version}`
      configLockfile.importers['.'].configDependencies[pkgName] = {
        specifier: version,
        version,
      }
      configLockfile.packages[pkgKey] = {
        resolution: toLockfileResolution(
          { name: pkgName, version },
          { integrity, tarball },
          registry
        ),
      }
      configLockfile.snapshots[pkgKey] = {}
      normalizedDeps[pkgName] = {
        version,
        resolution: { integrity, tarball },
      }
      continue
    }

    if (typeof pkgSpec === 'string') {
      // This branch only handles the legacy inline format (version+integrity).
      // New clean specifiers (just version/range) require an existing pnpm-config-lock.yaml.
      if (!pkgSpec.includes('+')) {
        throw new PnpmError(
          'CONFIG_DEP_MISSING_LOCKFILE',
          `Config dependency "${pkgName}" is already in clean-specifier form (${pkgSpec}) ` +
          'but no pnpm-config-lock.yaml was found to resolve it. ' +
          'Please generate and commit pnpm-config-lock.yaml (for example by running ' +
          '`pnpm install` in the workspace root) before attempting to migrate configDependencies.'
        )
      }
      const { version, integrity } = parseIntegrity(pkgName, pkgSpec)
      const tarball = getNpmTarballUrl(pkgName, version, { registry })

      cleanSpecifiers[pkgName] = version
      const pkgKey = `${pkgName}@${version}`
      configLockfile.importers['.'].configDependencies[pkgName] = {
        specifier: version,
        version,
      }
      configLockfile.packages[pkgKey] = {
        resolution: { integrity },
      }
      configLockfile.snapshots[pkgKey] = {}
      normalizedDeps[pkgName] = {
        version,
        resolution: { integrity, tarball },
      }
    }
  }

  // Write the new config lockfile and clean up workspace manifest
  await Promise.all([
    writeConfigLockfile(opts.rootDir, configLockfile),
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

function parseIntegrity (pkgName: string, pkgSpec: string) {
  const sepIndex = pkgSpec.indexOf('+')
  if (sepIndex === -1) {
    throw new PnpmError('CONFIG_DEP_NO_INTEGRITY', `Your config dependency called "${pkgName}" doesn't have an integrity checksum`, {
      hint: `Integrity checksum should be inlined in the version specifier. For example:

pnpm-workspace.yaml:
configDependencies:
  my-config: "1.0.0+sha512-Xg0tn4HcfTijTwfDwYlvVCl43V6h4KyVVX2aEm4qdO/PC6L2YvzLHFdmxhoeSA3eslcE6+ZVXHgWwopXYLNq4Q=="
`,
    })
  }
  const version = pkgSpec.substring(0, sepIndex)
  const integrity = pkgSpec.substring(sepIndex + 1)
  return { version, integrity }
}
