import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { writeSettings } from '@pnpm/config.writer'
import { PnpmError } from '@pnpm/error'
import {
  createEnvLockfile,
  type EnvLockfile,
  readEnvLockfile,
  writeEnvLockfile,
} from '@pnpm/lockfile.fs'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import { createNpmResolver, type ResolverFactoryOptions } from '@pnpm/resolving.npm-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { Creds, ConfigDependencies, ConfigDependencySpecifiers } from '@pnpm/types'

import { installConfigDeps, type InstallConfigDepsOpts } from './installConfigDeps.js'

export type ResolveConfigDepsOpts = CreateFetchFromRegistryOptions & ResolverFactoryOptions & InstallConfigDepsOpts & {
  configDependencies?: ConfigDependencies
  rootDir: string
  authInfos?: Record<string, Creds>
}

export async function resolveConfigDeps (configDeps: string[], opts: ResolveConfigDepsOpts): Promise<void> {
  if (opts.frozenLockfile) {
    throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE', 'Cannot resolve configDependencies with "frozen-lockfile" because the lockfile is not up to date')
  }

  const fetch = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.authInfos ?? {}, opts.registries?.default)
  const { resolveFromNpm } = createNpmResolver(fetch, getAuthHeader, opts)

  // Extract existing specifiers from configDependencies (handles both old and new formats)
  const configDependencySpecifiers: ConfigDependencySpecifiers = extractSpecifiers(opts.configDependencies)
  const envLockfile: EnvLockfile = (await readEnvLockfile(opts.rootDir)) ?? createEnvLockfile()

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
    const registry = pickRegistryForPackage(opts.registries, pkgName)

    // Write clean specifier to workspace manifest
    configDependencySpecifiers[pkgName] = wantedDep.bareSpecifier ?? version

    // Write resolved info to env lockfile
    const pkgKey = `${pkgName}@${version}`
    envLockfile.importers['.'].configDependencies[pkgName] = {
      specifier: configDependencySpecifiers[pkgName],
      version,
    }
    envLockfile.packages[pkgKey] = {
      resolution: toLockfileResolution(
        { name: pkgName, version },
        resolution.resolution,
        registry
      ),
    }
    envLockfile.snapshots[pkgKey] = {}
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
    writeEnvLockfile(opts.rootDir, envLockfile),
  ])
  await installConfigDeps(envLockfile, opts)
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
