import * as dp from 'dependency-path'
import {PackageSnapshot} from './types'

export default (
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
) => {
  if (!pkgSnapshot.name) {
    const pkgInfo = dp.parse(relDepPath)
    return {
      // tslint:disable:no-string-literal
      name: pkgInfo['name'],
      version: pkgInfo['version'],
      // tslint:enable:no-string-literal
    }
  }
  return {
    name: pkgSnapshot.name,
    version: pkgSnapshot.version,
  }
}
