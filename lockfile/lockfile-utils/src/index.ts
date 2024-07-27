import { refToRelative } from '@pnpm/dependency-path'

export { extendProjectsWithTargetDirs } from './extendProjectsWithTargetDirs'
export { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot'
export { packageIdFromSnapshot } from './packageIdFromSnapshot'
export { packageIsIndependent } from './packageIsIndependent'
export { pkgSnapshotToResolution } from './pkgSnapshotToResolution'
export { refIsLocalTarball, refIsLocalDirectory } from './refIsLocalTarball'
export * from '@pnpm/lockfile-types'

// for backward compatibility
export const getPkgShortId = refToRelative
