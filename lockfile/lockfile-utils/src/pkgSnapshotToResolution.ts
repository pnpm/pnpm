import url from 'node:url'

import getNpmTarballUrl from 'get-npm-tarball-url'

import * as dp from '@pnpm/dependency-path'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import type { Registries, Resolution, PackageSnapshot, TarballResolution } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

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

  let registry: string | undefined = name?.startsWith('@') ? registries[name.split('/')[0] ?? ''] : ''

  if (!registry) {
    registry = registries.default
  }

  const tarball: string = typeof pkgSnapshot.resolution !== 'undefined' && 'tarball' in pkgSnapshot.resolution
    ? new url.URL(
      pkgSnapshot.resolution.tarball,
      registry.endsWith('/') ? registry : `${registry}/`
    ).toString()
    : getTarball(registry);

  return {
    ...pkgSnapshot.resolution,
    tarball,
  }

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
