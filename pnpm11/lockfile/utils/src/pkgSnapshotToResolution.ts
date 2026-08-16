import url from 'node:url'

import { getRegistryServerType, normalizeNamedRegistries } from '@pnpm/config.normalize-registries'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import { getNpmTarballUrl } from '@pnpm/resolving.tarball-url'
import type { Registries, RegistryOptions } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export interface PkgSnapshotToResolutionOptions {
  registries: Registries
  /**
   * User-configured named registries (`namedRegistries` setting). Built-in
   * aliases are merged in here, so callers pass the setting verbatim.
   */
  namedRegistries?: Record<string, string>
  /**
   * Normalized `registryOptions` setting. The tarball URL is rebuilt with the
   * layout the registry it resolved from was declared to serve.
   */
  registryOptions?: Record<string, RegistryOptions>
}

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  opts: PkgSnapshotToResolutionOptions
): Resolution {
  const resolution = pkgSnapshot.resolution as TarballResolution
  if (resolution.tarball != null && typeof resolution.tarball !== 'string') {
    // Avoid URL string-coercion from malformed YAML lockfile values.
    throw new PnpmError('INVALID_TARBALL_RESOLUTION',
      `Cannot install package "${depPath}": its lockfile entry has a non-string "tarball" field.`)
  }
  if (
    Boolean(resolution.type) ||
    resolution.tarball?.startsWith('file:') ||
    resolution.gitHosted === true
  ) {
    return pkgSnapshot.resolution as Resolution
  }
  // Recover the tarball field for `file:` snapshots whose depPath is the only
  // source of the local tarball reference.
  const nonSemverVersion = dp.parse(depPath).nonSemverVersion
  if (nonSemverVersion?.startsWith('file:')) {
    return {
      ...pkgSnapshot.resolution,
      tarball: nonSemverVersion,
    } as Resolution
  }
  const { name, version, registryName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  let registry: string = ''
  if (registryName != null) {
    registry = normalizeNamedRegistries(opts.namedRegistries)[registryName]
    if (!registry) {
      throw new PnpmError('MISSING_NAMED_REGISTRY',
        `Cannot install package "${depPath}": it was resolved from the named registry '${registryName}:', which is not present in the namedRegistries setting.`,
        { hint: `Add '${registryName}' to the namedRegistries setting in pnpm-workspace.yaml.` })
    }
  } else if (name != null && name[0] === '@') {
    registry = opts.registries[name.split('/')[0]]
  }
  if (!registry) {
    registry = opts.registries.default
  }
  let tarball!: string
  if (!resolution.tarball) {
    tarball = getTarball(registry)
  } else {
    tarball = new url.URL(resolution.tarball,
      registry.endsWith('/') ? registry : `${registry}/`
    ).toString()
  }
  return {
    ...pkgSnapshot.resolution,
    tarball,
  } as Resolution

  function getTarball (registry: string) {
    if (!name || !version) {
      throw new Error(`Couldn't get tarball URL from dependency path ${depPath}`)
    }
    return getNpmTarballUrl(name, version, {
      registry,
      serverType: getRegistryServerType(opts.registryOptions, registry),
    })
  }
}
