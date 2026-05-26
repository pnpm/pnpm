import url from 'node:url'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import type { Registries } from '@pnpm/types'
import getNpmTarballUrl from 'get-npm-tarball-url'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  const resolution = pkgSnapshot.resolution as TarballResolution
  // Tarball-shaped resolutions (no `type` field) must carry `integrity`.
  // Without it the worker has nothing to verify the downloaded bytes
  // against and would mint a fresh integrity from whatever the URL
  // returned, so an attacker who can both alter the lockfile and serve
  // content at the referenced URL would otherwise have a tampered package
  // installed without detection. Pacquet enforces the same invariant via
  // `pacquet_package_manager::missing_tarball_integrity`.
  if (resolution.type == null && resolution.integrity == null) {
    throw new PnpmError('MISSING_TARBALL_INTEGRITY',
      `Cannot install package "${depPath}": its lockfile entry has no "integrity" field, so pnpm cannot verify the downloaded tarball.`,
      { hint: 'The lockfile may be corrupted or have been tampered with. Restore it from a trusted source, or delete it and re-run installation without --frozen-lockfile to regenerate.' }
    )
  }
  if (
    Boolean(resolution.type) ||
    resolution.tarball?.startsWith('file:') ||
    resolution.gitHosted === true
  ) {
    return pkgSnapshot.resolution as Resolution
  }
  // Recover the tarball field for `file:` snapshots whose resolution lost
  // its tarball (e.g. lockfiles written by an earlier pnpm 11 version that
  // dropped the tarball under `lockfile-include-tarball-url=false`).
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
  if (!(pkgSnapshot.resolution as TarballResolution).tarball) {
    tarball = getTarball(registry)
  } else {
    tarball = new url.URL((pkgSnapshot.resolution as TarballResolution).tarball,
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
