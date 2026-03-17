export * from './api.js'
export { UnexpectedStoreError } from './install/checkCompatibility/UnexpectedStoreError.js'
export { UnexpectedVirtualStoreDirError } from './install/checkCompatibility/UnexpectedVirtualStoreDirError.js'
export type { InstallOptions } from './install/extendInstallOptions.js'
export type { HoistingLimits } from '@pnpm/installing.deps-restorer'
export { type ProjectOptions } from '@pnpm/installing.get-context'
export type { UpdateMatchingFunction } from '@pnpm/installing.resolve-dependencies'
export type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
export type {
  BadPeerDependencyIssue,
  MissingPeerDependencyIssue,
  MissingPeerIssuesByPeerName,
  PackageManifest,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
} from '@pnpm/types'
