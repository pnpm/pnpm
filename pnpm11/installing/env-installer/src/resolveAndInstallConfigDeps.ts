import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import {
  createEnvLockfile,
  type EnvLockfile,
  readEnvLockfile,
} from '@pnpm/lockfile.fs'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import type { ConfigDependencies } from '@pnpm/types'

import { type ConfigDepResolverOpts, type ConfigDepResolvers, createConfigDepResolvers } from './createConfigDepResolvers.js'
import { installConfigDeps, type InstallConfigDepsOpts } from './installConfigDeps.js'
import { parseIntegrity } from './parseIntegrity.js'
import { pruneEnvLockfile } from './pruneEnvLockfile.js'
import { resolveOptionalSubdeps } from './resolveOptionalSubdeps.js'
import { createConfigDepsVerifier } from './verifyConfigDepResolutions.js'
import { assertValidMigratedConfigDep } from './verifyEnvLockfile.js'
import { writeVerifiedEnvLockfile } from './writeVerifiedEnvLockfile.js'

export type ResolveAndInstallConfigDepsOpts = ConfigDepResolverOpts & InstallConfigDepsOpts & {
  rootDir: string
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
  opts = { ...opts, now: opts.now ?? Date.now() }
  const verify = createConfigDepsVerifier(configDeps, opts)
  const envLockfile: EnvLockfile = (await readEnvLockfile(opts.rootDir)) ?? createEnvLockfile()
  const { depsToResolve, lockfileChanged } = collectConfigDepsToResolve(configDeps, {
    envLockfile,
    registriesByScope: opts.registriesByScope,
  })

  if (opts.frozenLockfile && (lockfileChanged || depsToResolve.length > 0)) {
    throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE', 'Cannot update configDependencies with "frozen-lockfile" because the lockfile is not up to date')
  }

  if (depsToResolve.length === 0) {
    await verify(envLockfile)
    if (lockfileChanged) {
      await writeVerifiedEnvLockfile(opts.rootDir, envLockfile)
    }
    await installConfigDeps(envLockfile, opts)
    return
  }

  const resolveCtx: ResolveConfigDepContext = { envLockfile, opts, resolvers: createConfigDepResolvers(opts) }

  await Promise.all(depsToResolve.map((dep) => resolveConfigDepIntoLockfile(resolveCtx, dep)))

  pruneEnvLockfile(envLockfile)

  await verify(envLockfile)
  await writeVerifiedEnvLockfile(opts.rootDir, envLockfile)
  await installConfigDeps(envLockfile, opts)
}

interface ConfigDepToResolve {
  name: string
  specifier: string
  pinnedIntegrity?: string
}

interface ConfigDepPlanContext {
  envLockfile: EnvLockfile
  registriesByScope: ResolveAndInstallConfigDepsOpts['registriesByScope']
}

/**
 * Written straight into the env lockfile, without resolving.
 */
const MIGRATED_INTO_LOCKFILE = 'migrated'

type ConfigDepPlan = ConfigDepToResolve | typeof MIGRATED_INTO_LOCKFILE | undefined

function collectConfigDepsToResolve (
  configDeps: ConfigDependencies,
  ctx: ConfigDepPlanContext
): { depsToResolve: ConfigDepToResolve[], lockfileChanged: boolean } {
  const depsToResolve: ConfigDepToResolve[] = []
  let lockfileChanged = false
  for (const [name, value] of Object.entries(configDeps)) {
    const plan = planConfigDep(ctx, name, value)
    if (plan === MIGRATED_INTO_LOCKFILE) {
      lockfileChanged = true
    } else if (plan) {
      depsToResolve.push(plan)
    }
  }
  return { depsToResolve, lockfileChanged }
}

function planConfigDep (ctx: ConfigDepPlanContext, name: string, value: ConfigDependencies[string]): ConfigDepPlan {
  const lockfileConfigDeps = ctx.envLockfile.importers['.'].configDependencies
  if (typeof value === 'object') {
    // Old object format — migrate inline into lockfile
    if (lockfileConfigDeps[name]) return undefined
    return planObjectFormatConfigDep(ctx, name, value)
  }

  if (value.includes('+')) {
    // Old string format with inline integrity — resolve its tarball URL, then migrate
    if (lockfileConfigDeps[name]) return undefined
    const { version, integrity } = parseIntegrity(name, value)
    assertValidMigratedConfigDep(name, version)
    return { name, specifier: version, pinnedIntegrity: integrity }
  }

  // New format (clean specifier like "1.2.0" or "^1.0.0")
  const existing = lockfileConfigDeps[name]
  if (existing && existing.specifier === value && ctx.envLockfile.packages[`${name}@${existing.version}`]) {
    return undefined // fully resolved
  }
  return { name, specifier: value }
}

function planObjectFormatConfigDep (
  ctx: ConfigDepPlanContext,
  name: string,
  value: Exclude<ConfigDependencies[string], string>
): ConfigDepPlan {
  const { version, integrity } = parseIntegrity(name, value.integrity)
  assertValidMigratedConfigDep(name, version)
  if (value.tarball == null) {
    return { name, specifier: version, pinnedIntegrity: integrity }
  }
  const registry = pickRegistryForPackage(ctx.registriesByScope, name)
  const pkgKey = `${name}@${version}`
  ctx.envLockfile.importers['.'].configDependencies[name] = { specifier: version, version }
  ctx.envLockfile.packages[pkgKey] = {
    resolution: toLockfileResolution({ name, version }, { integrity, tarball: value.tarball }, { registry }),
  }
  ctx.envLockfile.snapshots[pkgKey] = {}
  return MIGRATED_INTO_LOCKFILE
}

interface ResolveConfigDepContext {
  envLockfile: EnvLockfile
  opts: ResolveAndInstallConfigDepsOpts
  resolvers: ConfigDepResolvers
}

async function resolveConfigDepIntoLockfile (ctx: ResolveConfigDepContext, dep: ConfigDepToResolve): Promise<void> {
  const { name, specifier, pinnedIntegrity } = dep
  const { envLockfile, opts } = ctx
  const resolve = pinnedIntegrity == null ? ctx.resolvers.resolve : ctx.resolvers.resolvePinned
  const resolution = await resolve({ alias: name, bareSpecifier: specifier }, {
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
  const pkgKey = `${name}@${version}`

  envLockfile.importers['.'].configDependencies[name] = {
    specifier,
    version,
  }
  // A migrated dependency keeps the integrity pinned in pnpm-workspace.yaml,
  // so the registry hands over the tarball URL without loosening the pin.
  const pkgResolution = pinnedIntegrity == null
    ? resolution.resolution
    : { ...resolution.resolution, integrity: pinnedIntegrity }
  envLockfile.packages[pkgKey] = {
    resolution: toLockfileResolution({ name, version }, pkgResolution, {
      registry: pickRegistryForPackage(opts.registriesByScope, name),
    }),
  }
  // A pinned dependency covers only itself, so its optional subdeps stay out
  // of the lockfile until it is declared as a clean specifier.
  const optionalSubdeps = pinnedIntegrity == null
    ? await resolveOptionalSubdeps(name, resolution.manifest, {
      envLockfile,
      lockfileDir: opts.rootDir,
      registriesByScope: opts.registriesByScope,
      resolveFromNpm: ctx.resolvers.resolve,
    })
    : undefined
  envLockfile.snapshots[pkgKey] = optionalSubdeps ? { optionalDependencies: optionalSubdeps } : {}
}
