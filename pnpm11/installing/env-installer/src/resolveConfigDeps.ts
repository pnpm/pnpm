import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { writeSettings } from '@pnpm/config.writer'
import { PnpmError } from '@pnpm/error'
import {
  createEnvLockfile,
  type EnvLockfile,
  readEnvLockfile,
} from '@pnpm/lockfile.fs'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { ConfigDependencies, ConfigDependencySpecifiers } from '@pnpm/types'

import { type ConfigDepResolverOpts, createConfigDepResolvers, type ResolveConfigDep } from './createConfigDepResolvers.js'
import { installConfigDeps, type InstallConfigDepsOpts } from './installConfigDeps.js'
import { pruneEnvLockfile } from './pruneEnvLockfile.js'
import { resolveOptionalSubdeps } from './resolveOptionalSubdeps.js'
import { createConfigDepsVerifier } from './verifyConfigDepResolutions.js'
import { writeVerifiedEnvLockfile } from './writeVerifiedEnvLockfile.js'

export type ResolveConfigDepsOpts = ConfigDepResolverOpts & InstallConfigDepsOpts & {
  configDependencies?: ConfigDependencies
  rootDir: string
}

export async function resolveConfigDeps (configDeps: string[], opts: ResolveConfigDepsOpts): Promise<void> {
  if (opts.frozenLockfile) {
    throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE', 'Cannot resolve configDependencies with "frozen-lockfile" because the lockfile is not up to date')
  }

  opts = { ...opts, now: opts.now ?? Date.now() }
  const resolveFromNpm = createConfigDepResolvers(opts).resolve

  const configDependencySpecifiers: ConfigDependencySpecifiers = extractSpecifiers(opts.configDependencies)
  const envLockfile: EnvLockfile = (await readEnvLockfile(opts.rootDir)) ?? createEnvLockfile()

  const ctx: AddConfigDepContext = { configDependencySpecifiers, envLockfile, opts, resolveFromNpm }
  await Promise.all(configDeps.map((configDep) => addConfigDepToLockfile(ctx, configDep)))

  pruneEnvLockfile(envLockfile)

  await createConfigDepsVerifier(declaredConfigDepsExcept(opts.configDependencies, configDeps), opts)(envLockfile)
  await writeVerifiedEnvLockfile(opts.rootDir, envLockfile)
  await writeSettings({
    ...opts,
    rootProjectManifestDir: opts.rootDir,
    workspaceDir: opts.rootDir,
    updatedSettings: {
      configDependencies: configDependencySpecifiers,
    },
  })
  await installConfigDeps(envLockfile, opts)
}

interface AddConfigDepContext {
  configDependencySpecifiers: ConfigDependencySpecifiers
  envLockfile: EnvLockfile
  opts: ResolveConfigDepsOpts
  resolveFromNpm: ResolveConfigDep
}

async function addConfigDepToLockfile (ctx: AddConfigDepContext, configDep: string): Promise<void> {
  const { configDependencySpecifiers, envLockfile, opts } = ctx
  const wantedDep = parseWantedDependency(configDep)
  if (!wantedDep.alias) {
    throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency`)
  }
  const resolution = await ctx.resolveFromNpm(wantedDep, {
    lockfileDir: opts.rootDir,
    preferredVersions: {},
    projectDir: opts.rootDir,
  })
  if (resolution?.resolution == null || !('integrity' in resolution.resolution) || typeof resolution.resolution.integrity !== 'string' || !resolution.resolution.integrity) {
    throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency because it has no integrity`)
  }
  const pkgName = wantedDep.alias
  const version = resolution.manifest.version
  const registry = pickRegistryForPackage(opts.registriesByScope, pkgName)

  configDependencySpecifiers[pkgName] = wantedDep.bareSpecifier ?? version

  const pkgKey = `${pkgName}@${version}`
  envLockfile.importers['.'].configDependencies[pkgName] = {
    specifier: configDependencySpecifiers[pkgName],
    version,
  }
  envLockfile.packages[pkgKey] = {
    resolution: toLockfileResolution(
      { name: pkgName, version },
      resolution.resolution,
      { registry }
    ),
  }
  const optionalSubdeps = await resolveOptionalSubdeps(pkgName, resolution.manifest, {
    envLockfile,
    lockfileDir: opts.rootDir,
    registriesByScope: opts.registriesByScope,
    resolveFromNpm: ctx.resolveFromNpm,
  })
  envLockfile.snapshots[pkgKey] = optionalSubdeps ? { optionalDependencies: optionalSubdeps } : {}
}

function declaredConfigDepsExcept (configDependencies: ConfigDependencies | undefined, added: string[]): ConfigDependencies {
  const addedNames = new Set(added.map((configDep) => parseWantedDependency(configDep).alias))
  return Object.fromEntries(Object.entries(configDependencies ?? {}).filter(([name]) => !addedNames.has(name)))
}

/**
 * Extracts plain specifiers from configDependencies, handling both old format
 * ("version+integrity") and new format (plain specifiers).
 */
function extractSpecifiers (configDependencies?: ConfigDependencies): ConfigDependencySpecifiers {
  if (!configDependencies) return {}
  const specifiers: ConfigDependencySpecifiers = {}
  for (const [name, value] of Object.entries(configDependencies)) {
    // Old format with tarball: extract version from integrity string.
    // A string could be old "version+integrity" or new plain specifier.
    specifiers[name] = stripInlineIntegrity(typeof value === 'object' ? value.integrity : value)
  }
  return specifiers
}

function stripInlineIntegrity (spec: string): string {
  const sepIndex = spec.indexOf('+')
  return sepIndex !== -1 ? spec.substring(0, sepIndex) : spec
}
