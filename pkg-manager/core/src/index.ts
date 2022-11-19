export {
  BadPeerDependencyIssue,
  MissingPeerDependencyIssue,
  MissingPeerIssuesByPeerName,
  PackageManifest,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
} from '@pnpm/types'
export { HoistingLimits } from '@pnpm/headless'
export * from './api'
export * from './install/hooks'

export { ProjectOptions, UnexpectedStoreError, UnexpectedVirtualStoreDirError } from '@pnpm/get-context'
export { InstallOptions } from './install/extendInstallOptions'

export { WorkspacePackages } from '@pnpm/resolver-base'
