export * from '@pnpm/shrinkwrap-types'

import nameVerFromPkgSnapshot from './nameVerFromPkgSnapshot'
import packageIsIndependent from './packageIsIndependent'
import pkgSnapshotToResolution from './pkgSnapshotToResolution'
import satisfiesPackageJson from './satisfiesPackageJson'

export {
  nameVerFromPkgSnapshot,
  packageIsIndependent,
  pkgSnapshotToResolution,
  satisfiesPackageJson,
}

// for backward compatibility
import { refToRelative } from 'dependency-path'
export const getPkgShortId = refToRelative
