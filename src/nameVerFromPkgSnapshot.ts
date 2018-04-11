import * as dp from 'dependency-path'
import {PackageSnapshot} from './types'

export default (
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
) => {
  if (!pkgSnapshot.name) {
    const pkgInfo = dp.parse(relDepPath)
    return {
      name: pkgInfo.name as string,
      version: pkgInfo.version as string,
    }
  }
  return {
    name: pkgSnapshot.name,
    version: pkgSnapshot.version as string,
  }
}
