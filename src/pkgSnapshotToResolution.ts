import {Resolution} from '@pnpm/resolver-base'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import url = require('url')
import {PackageSnapshot} from './types'

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
    const parsed = dp.parse(relDepPath)
    if (!parsed.name || !parsed.version) {
      throw new Error(`Couldn't get tarball URL from dependency path ${relDepPath}`)
    }
    return getNpmTarballUrl(parsed.name, parsed.version, {registry})
  }
  // tslint:enable:no-string-literal
}
