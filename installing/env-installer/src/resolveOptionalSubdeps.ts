import util from 'node:util'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.fs'
import type { ResolvedDependencies } from '@pnpm/lockfile.types'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import type { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { DependencyManifest, Registries } from '@pnpm/types'
import semver from 'semver'

type ResolveFromNpm = ReturnType<typeof createNpmResolver>['resolveFromNpm']

export interface ResolveOptionalSubdepsOpts {
  envLockfile: EnvLockfile
  lockfileDir: string
  registries: Registries
  resolveFromNpm: ResolveFromNpm
}

export async function resolveOptionalSubdeps (
  parentName: string,
  parentManifest: DependencyManifest,
  opts: ResolveOptionalSubdepsOpts
): Promise<ResolvedDependencies | undefined> {
  const optionalDeps = parentManifest.optionalDependencies
  if (!optionalDeps || Object.keys(optionalDeps).length === 0) {
    return undefined
  }

  const resolved: ResolvedDependencies = {}
  await Promise.all(Object.entries(optionalDeps).map(async ([subdepName, subdepSpec]) => {
    if (semver.valid(subdepSpec) == null) {
      // Ranges and tags would let the resolved version drift between machines
      // even with a stable parent integrity, breaking the lockfile's promise
      // of reproducible config-dep installs.
      throw new PnpmError(
        'CONFIG_DEP_OPTIONAL_NOT_EXACT',
        `Cannot install "${subdepName}@${subdepSpec}" as an optionalDependency of config dependency "${parentName}": only exact versions are supported (got "${subdepSpec}")`
      )
    }
    let resolution
    try {
      // `optional: true` opts into full registry metadata so the resolver
      // returns `libc` (and any other fields the abbreviated metadata strips).
      // See pnpm/pnpm#9950.
      resolution = await opts.resolveFromNpm({ alias: subdepName, bareSpecifier: subdepSpec, optional: true }, {
        lockfileDir: opts.lockfileDir,
        preferredVersions: {},
        projectDir: opts.lockfileDir,
      })
    } catch (err: unknown) {
      // Trust-downgrade is a security signal that must fail the install even
      // for optional deps; everything else mirrors npm's optionalDependencies
      // semantics — log and skip.
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_TRUST_DOWNGRADE') {
        throw err
      }
      skippedOptionalDependencyLogger.debug({
        details: util.types.isNativeError(err) ? err.toString() : String(err),
        package: {
          name: subdepName,
          // No resolved version yet; surface the requested specifier so log
          // consumers that format `${name}@${version}` don't render `@undefined`.
          version: subdepSpec,
          bareSpecifier: subdepSpec,
        },
        parents: [{ id: `${parentName}@${parentManifest.version}`, name: parentName, version: parentManifest.version }],
        prefix: opts.lockfileDir,
        reason: 'resolution_failure',
      })
      return
    }
    if (
      resolution?.resolution == null ||
      !('integrity' in resolution.resolution) ||
      typeof resolution.resolution.integrity !== 'string' ||
      !resolution.resolution.integrity ||
      resolution.manifest == null
    ) {
      throw new PnpmError(
        'BAD_CONFIG_DEP',
        `Cannot resolve optionalDependency "${subdepName}" of config dependency "${parentName}" because it has no integrity`
      )
    }
    const subdepVersion = resolution.manifest.version
    const registry = pickRegistryForPackage(opts.registries, subdepName)
    const subdepKey = `${subdepName}@${subdepVersion}`

    opts.envLockfile.packages[subdepKey] = {
      resolution: toLockfileResolution(
        { name: subdepName, version: subdepVersion },
        resolution.resolution,
        registry
      ),
      ...pickPlatformFields(resolution.manifest),
    }
    if (opts.envLockfile.snapshots[subdepKey] == null) {
      opts.envLockfile.snapshots[subdepKey] = { optional: true }
    }
    resolved[subdepName] = subdepVersion
  }))

  return Object.keys(resolved).length > 0 ? resolved : undefined
}

function pickPlatformFields (manifest: DependencyManifest): { os?: string[], cpu?: string[], libc?: string[] } {
  const out: { os?: string[], cpu?: string[], libc?: string[] } = {}
  if (manifest.os?.length) out.os = manifest.os
  if (manifest.cpu?.length) out.cpu = manifest.cpu
  if (manifest.libc?.length) out.libc = manifest.libc
  return out
}
