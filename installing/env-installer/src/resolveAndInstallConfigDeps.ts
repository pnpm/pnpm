import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
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
import type { Creds, ConfigDependencies } from '@pnpm/types'
import getNpmTarballUrl from 'get-npm-tarball-url'

import { installConfigDeps, type InstallConfigDepsOpts } from './installConfigDeps.js'
import { parseIntegrity } from './parseIntegrity.js'
import { pruneEnvLockfile } from './pruneEnvLockfile.js'

export type ResolveAndInstallConfigDepsOpts = CreateFetchFromRegistryOptions & ResolverFactoryOptions & InstallConfigDepsOpts & {
  rootDir: string
  authInfos?: Record<string, Creds>
}

/**
 * Resolves any config dependencies that are missing from the env lockfile,
 * then installs all config dependencies.
 *
 * This handles two scenarios:
 * 1. User manually added config deps to pnpm-workspace.yaml
 * 2. User deleted pnpm-lock.yaml after installing config deps
 */
export async function resolveAndInstallConfigDeps (
  configDeps: ConfigDependencies,
  opts: ResolveAndInstallConfigDepsOpts
): Promise<void> {
  const envLockfile: EnvLockfile = (await readEnvLockfile(opts.rootDir)) ?? createEnvLockfile()
  const lockfileConfigDeps = envLockfile.importers['.'].configDependencies

  const depsToResolve: Array<{ name: string, specifier: string }> = []
  let lockfileChanged = false

  for (const [name, value] of Object.entries(configDeps)) {
    if (typeof value === 'object') {
      // Old object format — migrate inline into lockfile
      if (!lockfileConfigDeps[name]) {
        const registry = pickRegistryForPackage(opts.registries, name)
        const { version, integrity } = parseIntegrity(name, value.integrity)
        const tarball = value.tarball ?? getNpmTarballUrl(name, version, { registry })
        const pkgKey = `${name}@${version}`
        lockfileConfigDeps[name] = { specifier: version, version }
        envLockfile.packages[pkgKey] = {
          resolution: toLockfileResolution({ name, version }, { integrity, tarball }, registry),
        }
        envLockfile.snapshots[pkgKey] = {}
        lockfileChanged = true
      }
      continue
    }

    if (value.includes('+')) {
      // Old string format with inline integrity — migrate into lockfile
      if (!lockfileConfigDeps[name]) {
        const registry = pickRegistryForPackage(opts.registries, name)
        const { version, integrity } = parseIntegrity(name, value)
        const tarball = getNpmTarballUrl(name, version, { registry })
        const pkgKey = `${name}@${version}`
        lockfileConfigDeps[name] = { specifier: version, version }
        envLockfile.packages[pkgKey] = {
          resolution: toLockfileResolution({ name, version }, { integrity, tarball }, registry),
        }
        envLockfile.snapshots[pkgKey] = {}
        lockfileChanged = true
      }
      continue
    }

    // New format (clean specifier like "1.2.0" or "^1.0.0")
    const specifier = value
    const existing = lockfileConfigDeps[name]
    if (existing && existing.specifier === specifier) {
      const pkgKey = `${name}@${existing.version}`
      if (envLockfile.packages[pkgKey]) continue // fully resolved
    }
    depsToResolve.push({ name, specifier })
  }

  if (opts.frozenLockfile && (lockfileChanged || depsToResolve.length > 0)) {
    throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE', 'Cannot update configDependencies with "frozen-lockfile" because the lockfile is not up to date')
  }

  if (depsToResolve.length === 0) {
    if (lockfileChanged) {
      await writeEnvLockfile(opts.rootDir, envLockfile)
    }
    await installConfigDeps(envLockfile, opts)
    return
  }

  // Resolve missing deps
  const fetch = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.authInfos ?? {}, opts.registries?.default)
  const { resolveFromNpm } = createNpmResolver(fetch, getAuthHeader, opts)

  await Promise.all(depsToResolve.map(async ({ name, specifier }) => {
    const resolution = await resolveFromNpm({ alias: name, bareSpecifier: specifier }, {
      lockfileDir: opts.rootDir,
      preferredVersions: {},
      projectDir: opts.rootDir,
    })
    if (
      resolution?.resolution == null ||
      !('integrity' in resolution.resolution) ||
      typeof resolution.resolution.integrity !== 'string' ||
      !resolution.resolution.integrity
    ) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot resolve ${name}@${specifier} as a configuration dependency because it has no integrity`)
    }
    const version = resolution.manifest.version
    const registry = pickRegistryForPackage(opts.registries, name)
    const pkgKey = `${name}@${version}`

    lockfileConfigDeps[name] = {
      specifier,
      version,
    }
    envLockfile.packages[pkgKey] = {
      resolution: toLockfileResolution(
        { name, version },
        resolution.resolution,
        registry
      ),
    }
    envLockfile.snapshots[pkgKey] = {}
  }))

  pruneEnvLockfile(envLockfile)

  await writeEnvLockfile(opts.rootDir, envLockfile)
  await installConfigDeps(envLockfile, opts)
}
