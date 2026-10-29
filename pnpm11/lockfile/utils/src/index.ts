import { refToRelative } from '@pnpm/deps.path'

export { findLockedRootNodeRuntime, type LockedNodeRuntime } from './findLockedRootNodeRuntime.js'
export { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'
export { packageIdFromSnapshot } from './packageIdFromSnapshot.js'
export { packageIsIndependent } from './packageIsIndependent.js'
export { pkgSnapshotToResolution, type PkgSnapshotToResolutionOptions } from './pkgSnapshotToResolution.js'
export { refIsLocalDirectory, refIsLocalTarball, resolveLocalTarballPath } from './refIsLocalTarball.js'
export {
  findMovedRegistryKey,
  findOutdatedRegistryKey,
  keyPrefixForAlias,
  keyPrefixForRegistry,
  type MovedRegistryKey,
  type OutdatedRegistryKey,
  registryOfDepPath,
  registryOfPackage,
  registryOfPrefix,
  rekeyDepPath,
} from './registryKeys.js'
export { resolvedPackageVersionsFromLockfile } from './resolvedPackageVersions.js'
export { toLockfileResolution } from './toLockfileResolution.js'
export * from '@pnpm/lockfile.types'
export { isGitHostedTarballUrl } from '@pnpm/resolving.resolver-base'

// for backward compatibility
export const getPkgShortId = refToRelative
