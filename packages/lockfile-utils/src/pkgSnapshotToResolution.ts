import { PackageSnapshot } from '@pnpm/lockfile-types'
import { Resolution } from '@pnpm/resolver-base'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import nameVerFromPkgSnapshot from './nameVerFromPkgSnapshot'
import url = require('url')

export default (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution => {
  /* eslint-disable @typescript-eslint/dot-notation */
  if (pkgSnapshot.resolution['type'] || pkgSnapshot.resolution['tarball']?.startsWith('file:')) {
    return pkgSnapshot.resolution as Resolution
  }
  const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const registry = pkgSnapshot.resolution['registry'] ||
    (name[0] === '@' && registries[name.split('/')[0]]) ||
    registries.default
  let tarball!: string
  if (!pkgSnapshot.resolution['tarball']) {
    tarball = getTarball(registry)
  } else {
    tarball = url.resolve(registry, pkgSnapshot.resolution['tarball'])
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
  /* eslint-enable @typescript-eslint/dot-notation */
}
