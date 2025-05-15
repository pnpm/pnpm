export type {
  BadPeerDependencyIssue,
  MissingPeerDependencyIssue,
  MissingPeerIssuesByPeerName,
  PackageManifest,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
} from '@pnpm/types'
export type { HoistingLimits } from '@pnpm/headless'
export * from './api'

export { type ProjectOptions } from '@pnpm/get-context'
export { UnexpectedStoreError } from './install/checkCompatibility/UnexpectedStoreError'
export { UnexpectedVirtualStoreDirError } from './install/checkCompatibility/UnexpectedVirtualStoreDirError'
export type { InstallOptions } from './install/extendInstallOptions'

export type { WorkspacePackages } from '@pnpm/resolver-base'
export type { UpdateMatchingFunction } from '@pnpm/resolve-dependencies'
