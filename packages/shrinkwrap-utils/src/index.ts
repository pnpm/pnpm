export * from '@pnpm/shrinkwrap-types'

import nameVerFromPkgSnapshot from './nameVerFromPkgSnapshot'
import pkgSnapshotToResolution from './pkgSnapshotToResolution'
import satisfiesPackageJson from './satisfiesPackageJson'

export {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
  satisfiesPackageJson,
}

// for backward compatibility
import { refToRelative } from 'dependency-path'
export const getPkgShortId = refToRelative
