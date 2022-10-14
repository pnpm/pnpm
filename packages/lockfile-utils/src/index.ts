import { refToRelative } from 'dependency-path'

export { extendProjectsWithTargetDirs } from './extendProjectsWithTargetDirs'
export { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot'
export { packageIdFromSnapshot } from './packageIdFromSnapshot'
export { packageIsIndependent } from './packageIsIndependent'
export { pkgSnapshotToResolution } from './pkgSnapshotToResolution'
export { satisfiesPackageManifest } from './satisfiesPackageManifest'
export * from '@pnpm/lockfile-types'

// for backward compatibility
export const getPkgShortId = refToRelative
