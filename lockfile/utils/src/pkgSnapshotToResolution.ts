import url from 'node:url'

import * as dp from '@pnpm/deps.path'
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
  if (
    Boolean((pkgSnapshot.resolution as TarballResolution).type) ||
    (pkgSnapshot.resolution as TarballResolution).tarball?.startsWith('file:') ||
    (pkgSnapshot.resolution as TarballResolution).gitHosted === true
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
