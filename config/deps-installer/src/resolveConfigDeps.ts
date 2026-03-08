import getNpmTarballUrl from 'get-npm-tarball-url'
import { PnpmError } from '@pnpm/error'
import { writeSettings } from '@pnpm/config.config-writer'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/fetch'
import { createNpmResolver, type ResolverFactoryOptions } from '@pnpm/npm-resolver'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import type { ConfigDependencies, ConfigDependencySpecifiers } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { installConfigDeps, type InstallConfigDepsOpts } from './installConfigDeps.js'
import { type ConfigLockfile, createConfigLockfile, readConfigLockfile, writeConfigLockfile } from './configLockfile.js'

export type ResolveConfigDepsOpts = CreateFetchFromRegistryOptions & ResolverFactoryOptions & InstallConfigDepsOpts & {
  configDependencies?: ConfigDependencies
  rootDir: string
  userConfig?: Record<string, string>
}

export async function resolveConfigDeps (configDeps: string[], opts: ResolveConfigDepsOpts): Promise<void> {
  const fetch = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.userConfig!, userSettings: opts.userConfig })
  const { resolveFromNpm } = createNpmResolver(fetch, getAuthHeader, opts)

  // Extract existing specifiers from configDependencies (handles both old and new formats)
  const configDependencySpecifiers: ConfigDependencySpecifiers = extractSpecifiers(opts.configDependencies)
  const configLockfile: ConfigLockfile = (await readConfigLockfile(opts.rootDir)) ?? createConfigLockfile()

  await Promise.all(configDeps.map(async (configDep) => {
    const wantedDep = parseWantedDependency(configDep)
    if (!wantedDep.alias) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency`)
    }
    const resolution = await resolveFromNpm(wantedDep, {
      lockfileDir: opts.rootDir,
      preferredVersions: {},
      projectDir: opts.rootDir,
    })
    if (resolution?.resolution == null || !('integrity' in resolution.resolution) || typeof resolution.resolution.integrity !== 'string' || !resolution.resolution.integrity) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency because it has no integrity`)
    }
    const pkgName = wantedDep.alias
    const version = resolution.manifest.version
    const { tarball, integrity } = resolution.resolution as { tarball: string, integrity: string }
    const registry = pickRegistryForPackage(opts.registries, pkgName)
    const defaultTarball = getNpmTarballUrl(pkgName, version, { registry })
    const hasCustomTarball = tarball !== defaultTarball && isValidHttpUrl(tarball)

    // Write clean specifier to workspace manifest
    configDependencySpecifiers[pkgName] = wantedDep.bareSpecifier ?? version

    // Write resolved info to config lockfile
    const pkgKey = `${pkgName}@${version}`
    configLockfile.importers['.'].configDependencies[pkgName] = {
      specifier: configDependencySpecifiers[pkgName],
      version,
    }
    configLockfile.packages[pkgKey] = {
      resolution: hasCustomTarball
        ? { integrity, tarball }
        : { integrity },
    }
    configLockfile.snapshots[pkgKey] = {}
  }))

  await Promise.all([
    writeSettings({
      ...opts,
      rootProjectManifestDir: opts.rootDir,
      workspaceDir: opts.rootDir,
      updatedSettings: {
        configDependencies: configDependencySpecifiers,
      },
    }),
    writeConfigLockfile(opts.rootDir, configLockfile),
  ])
  await installConfigDeps(configLockfile, opts)
}

/**
 * Extracts plain specifiers from configDependencies, handling both old format
 * ("version+integrity") and new format (plain specifiers).
 */
function extractSpecifiers (configDependencies?: ConfigDependencies): ConfigDependencySpecifiers {
  if (!configDependencies) return {}
  const specifiers: ConfigDependencySpecifiers = {}
  for (const [name, value] of Object.entries(configDependencies)) {
    if (typeof value === 'object') {
      // Old format with tarball: extract version from integrity string
      const sepIndex = value.integrity.indexOf('+')
      specifiers[name] = sepIndex !== -1 ? value.integrity.substring(0, sepIndex) : value.integrity
    } else {
      // Could be old "version+integrity" or new plain specifier
      const sepIndex = value.indexOf('+')
      specifiers[name] = sepIndex !== -1 ? value.substring(0, sepIndex) : value
    }
  }
  return specifiers
}

function isValidHttpUrl (url: string): boolean {
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}
