import url from 'url'
import { type PackageSnapshot, type TarballResolution } from '@pnpm/lockfile-types'
import { type Resolution } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  if (
    Boolean((pkgSnapshot.resolution as TarballResolution).type) ||
    (pkgSnapshot.resolution as TarballResolution).tarball?.startsWith('file:') ||
    isGitHostedPkgUrl((pkgSnapshot.resolution as TarballResolution).tarball ?? '')
  ) {
    return pkgSnapshot.resolution as Resolution
  }
  const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  let registry: string = ''
  if (name != null) {
    if (name.startsWith('@')) {
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
    const { name, version } = dp.parse(depPath)
    if (!name || !version) {
      throw new Error(`Couldn't get tarball URL from dependency path ${depPath}`)
    }
    return getNpmTarballUrl(name, version, { registry })
  }
}
