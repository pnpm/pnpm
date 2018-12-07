import { Resolution } from '@pnpm/resolver-base'
import { PackageSnapshot } from '@pnpm/shrinkwrap-types'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import url = require('url')

export default (
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
  registry: string,
): Resolution => {
  // tslint:disable:no-string-literal
  if (pkgSnapshot.resolution['type']) {
    return pkgSnapshot.resolution as Resolution
  }
  if (!pkgSnapshot.resolution['tarball']) {
    return {
      ...pkgSnapshot.resolution,
      registry: pkgSnapshot.resolution['registry'] || registry,
      tarball: getTarball(),
    } as Resolution
  }
  if (pkgSnapshot.resolution['tarball'].startsWith('file:')) {
    return pkgSnapshot.resolution as Resolution
  }
  return {
    ...pkgSnapshot.resolution,
    registry: pkgSnapshot.resolution['registry'] || registry,
    tarball: url.resolve(registry, pkgSnapshot.resolution['tarball']),
  } as Resolution

  function getTarball () {
    const { name, version } = dp.parse(relDepPath)
    if (!name || !version) {
      throw new Error(`Couldn't get tarball URL from dependency path ${relDepPath}`)
    }
    return getNpmTarballUrl(name, version, { registry })
  }
  // tslint:enable:no-string-literal
}
