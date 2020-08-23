import { refToRelative } from 'dependency-path'
import nameVerFromPkgSnapshot from './nameVerFromPkgSnapshot'
import packageIdFromSnapshot from './packageIdFromSnapshot'
import packageIsIndependent from './packageIsIndependent'
import pkgSnapshotToResolution from './pkgSnapshotToResolution'
import satisfiesPackageManifest from './satisfiesPackageManifest'

export * from '@pnpm/lockfile-types'

export {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  packageIsIndependent,
  pkgSnapshotToResolution,
  satisfiesPackageManifest,
}

// for backward compatibility
export const getPkgShortId = refToRelative
