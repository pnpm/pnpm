import { refToRelative } from '@pnpm/dependency-path'

export { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'
export { packageIdFromSnapshot } from './packageIdFromSnapshot.js'
export { packageIsIndependent } from './packageIsIndependent.js'
export { pkgSnapshotToResolution } from './pkgSnapshotToResolution.js'
export { refIsLocalTarball, refIsLocalDirectory } from './refIsLocalTarball.js'
export * from '@pnpm/lockfile.types'

// for backward compatibility
export const getPkgShortId = refToRelative
