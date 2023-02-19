import url from 'url'
import { PackageSnapshot, TarballResolution } from '@pnpm/lockfile-types'
import { Resolution } from '@pnpm/resolver-base'
import { Registries } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  if (
    Boolean((pkgSnapshot.resolution as TarballResolution).type) ||
    (pkgSnapshot.resolution as TarballResolution).tarball?.startsWith('file:')
  ) {
    return pkgSnapshot.resolution as Resolution
  }
  const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const registry: string = (name[0] === '@' && registries[name.split('/')[0]]) || registries.default
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
    registry,
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
