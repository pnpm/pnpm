import * as dp from 'dependency-path'
import {DependencyShrinkwrap} from 'pnpm-shrinkwrap'

export default function getPkgInfoFromShr (
  relativeDepPath: string,
  pkgShr: DependencyShrinkwrap,
) {
  if (!pkgShr.name) {
    const pkgInfo = dp.parse(relativeDepPath)
    return {
      // tslint:disable:no-string-literal
      name: pkgInfo['name'],
      version: pkgInfo['version'],
      // tslint:enable:no-string-literal
    }
  }
  return {
    name: pkgShr.name,
    version: pkgShr.version,
  }
}
