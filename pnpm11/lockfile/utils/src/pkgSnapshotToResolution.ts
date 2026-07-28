import url from 'node:url'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import {
  getIntegrityAddressedTarballUrl,
  getNpmTarballUrl,
  isIntegrityAddressedRegistryTarballUrl,
  isValidTarballRevision,
} from '@pnpm/resolving.tarball-url'
import type { Registries } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  const resolution = pkgSnapshot.resolution as TarballResolution
  if (resolution.tarball != null && typeof resolution.tarball !== 'string') {
    // Avoid URL string-coercion from malformed YAML lockfile values.
    throw new PnpmError('INVALID_TARBALL_RESOLUTION',
      `Cannot install package "${depPath}": its lockfile entry has a non-string "tarball" field.`)
  }
  if (resolution.revision != null && !isValidTarballRevision(resolution.revision)) {
    throw new PnpmError('INVALID_TARBALL_REVISION',
      `Cannot install package "${depPath}": its lockfile entry has an invalid "revision" field.`)
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
  const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  let registry: string = ''
  if (name != null) {
    if (name[0] === '@') {
      registry = registries[name.split('/')[0]]
    }
  }
  if (!registry) {
    registry = registries.default
  }
  let tarball!: string
  if (!resolution.tarball) {
    if (resolution.revision == null) {
      tarball = getTarball(registry)
    } else {
      const integrityTarball = resolution.integrity == null
        ? undefined
        : getIntegrityAddressedTarballUrl(resolution.integrity, registry)
      if (integrityTarball == null) {
        throw new PnpmError('INVALID_TARBALL_REVISION',
          `Cannot install package "${depPath}": its lockfile entry with a revision has invalid or missing integrity.`)
      }
      tarball = integrityTarball
    }
  } else {
    if (
      resolution.revision != null &&
      (
        resolution.integrity == null ||
        !isIntegrityAddressedRegistryTarballUrl(resolution.tarball, resolution.integrity, registry)
      )
    ) {
      throw new PnpmError('INVALID_TARBALL_REVISION',
        `Cannot install package "${depPath}": its lockfile entry with a revision has a mismatched tarball URL.`)
    }
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
    return getNpmTarballUrl(name, version, { registry })
  }
}
