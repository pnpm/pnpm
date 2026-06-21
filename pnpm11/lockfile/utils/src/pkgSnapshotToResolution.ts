import url from 'node:url'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import { type Resolution, resolutionNeedsIntegrity } from '@pnpm/resolving.resolver-base'
import { getNpmTarballUrl } from '@pnpm/resolving.tarball-url'
import type { Registries } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  const resolution = pkgSnapshot.resolution as TarballResolution
  // Tarball-shaped resolutions (no `type` field) must carry `integrity`,
  // except where the URL itself anchors the bytes:
  //   - `file:` tarballs (local file on the user's machine; integrity
  //     adds nothing the user doesn't already control).
  //   - Git-hosted tarballs (URL contains the commit SHA; git's content-
  //     addressed model binds the bytes to the commit). The `gitHosted`
  //     flag may be absent on legacy lockfiles, so fall back to a URL
  //     match — same logic as `toLockfileResolution`.
  // For any other tarball entry a missing integrity is what a tampered
  // lockfile looks like: the worker would mint a fresh integrity from
  // whatever bytes the URL returned, so we fail closed here. Pacquet
  // enforces the same invariant via
  // `pacquet_package_manager::missing_tarball_integrity`.
  if (resolutionNeedsIntegrity(resolution)) {
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
