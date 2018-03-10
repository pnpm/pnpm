// TODO: move to separate package because it is used in supi resolveDependencies()
import {Resolution} from '@pnpm/resolver-base'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import {PackageSnapshot} from 'pnpm-shrinkwrap'
import url = require('url')

export default function dependencyShrToResolution (
  relDepPath: string,
  depShr: PackageSnapshot,
  registry: string,
): Resolution {
  // tslint:disable:no-string-literal
  if (depShr.resolution['type']) {
    return depShr.resolution as Resolution
  }
  if (!depShr.resolution['tarball']) {
    return {
      ...depShr.resolution,
      registry: depShr.resolution['registry'] || registry,
      tarball: getTarball(),
    } as Resolution
  }
  if (depShr.resolution['tarball'].startsWith('file:')) {
    return depShr.resolution as Resolution
  }
  return {
    ...depShr.resolution,
    tarball: url.resolve(registry, depShr.resolution['tarball']),
  } as Resolution

  function getTarball () {
    const parsed = dp.parse(relDepPath)
    if (!parsed['name'] || !parsed['version']) {
      throw new Error(`Couldn't get tarball URL from dependency path ${relDepPath}`)
    }
    return getNpmTarballUrl(parsed['name'], parsed['version'], {registry})
  }
  // tslint:enable:no-string-literal
}
