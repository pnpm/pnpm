import '@total-typescript/ts-reset'

import { refToRelative } from '@pnpm/dependency-path'

export { extendProjectsWithTargetDirs } from './extendProjectsWithTargetDirs.js'
export { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'
export { packageIdFromSnapshot } from './packageIdFromSnapshot.js'
export { packageIsIndependent } from './packageIsIndependent.js'
export { pkgSnapshotToResolution } from './pkgSnapshotToResolution.js'
export { satisfiesPackageManifest } from './satisfiesPackageManifest.js'
export { refIsLocalTarball, refIsLocalDirectory } from './refIsLocalTarball.js'

// for backward compatibility
export const getPkgShortId = refToRelative
