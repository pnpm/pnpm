export * from '@pnpm/lockfile-types'

import nameVerFromPkgSnapshot from './nameVerFromPkgSnapshot'
import packageIdFromSnapshot from './packageIdFromSnapshot'
import packageIsIndependent from './packageIsIndependent'
import pkgSnapshotToResolution from './pkgSnapshotToResolution'
import satisfiesPackageJson from './satisfiesPackageJson'

export {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  packageIsIndependent,
  pkgSnapshotToResolution,
  satisfiesPackageJson,
}

// for backward compatibility
import { refToRelative } from 'dependency-path'
export const getPkgShortId = refToRelative
