import * as dp from 'dependency-path'
import {DependencyShrinkwrap} from 'pnpm-shrinkwrap'

export default function getPkgInfoFromShr (
  relativeDepPath: string,
  pkgShr: DependencyShrinkwrap
) {
  if (!pkgShr.name) {
    const pkgInfo = dp.parse(relativeDepPath)
    return {
      name: pkgInfo['name'],
      version: pkgInfo['version'],
    }
  }
  return {
    name: pkgShr.name,
    version: pkgShr.version,
  }
}
