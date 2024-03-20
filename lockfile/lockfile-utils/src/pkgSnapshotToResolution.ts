import url from 'node:url'

import getNpmTarballUrl from 'get-npm-tarball-url'

import * as dp from '@pnpm/dependency-path'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import type { Registries, Resolution, PackageSnapshot, TarballResolution } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot'

export function pkgSnapshotToResolution(
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  if (
    Boolean((pkgSnapshot.resolution as TarballResolution).type) ||
    (pkgSnapshot.resolution as TarballResolution).tarball?.startsWith(
      'file:'
    ) ||
    isGitHostedPkgUrl(
      (pkgSnapshot.resolution as TarballResolution).tarball ?? ''
    )
  ) {
    return pkgSnapshot.resolution as Resolution
  }

  const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

  let registry: string = name?.startsWith('@') ? registries[name.split('/')[0]] : ''

  if (!registry) {
    registry = registries.default
  }

  const tarball: string = (pkgSnapshot.resolution as TarballResolution).tarball
    ? new url.URL(
      (pkgSnapshot.resolution as TarballResolution).tarball,
      registry.endsWith('/') ? registry : `${registry}/`
    ).toString()
    : getTarball(registry);

  return {
    ...pkgSnapshot.resolution,
    tarball,
  } as Resolution

  function getTarball(registry: string): string {
    const parsed = dp.parse(depPath)

    if (!('name' in parsed) || !parsed.name || !parsed.version) {
      throw new Error(
        `Couldn't get tarball URL from dependency path ${depPath}`
      )
    }

    return getNpmTarballUrl(parsed.name, parsed.version, { registry })
  }
}
