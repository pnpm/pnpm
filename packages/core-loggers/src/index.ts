import {
  type ContextLog,
  type DeprecationLog,
  type FetchingProgressLog,
  type ExecutionTimeLog,
  type HookLog,
  type InstallCheckLog,
  type IgnoredScriptsLog,
  type LifecycleLog,
  type LinkLog,
  type PackageImportMethodLog,
  type PackageManifestLog,
  type PeerDependencyIssuesLog,
  type ProgressLog,
  type RegistryLog,
  type RequestRetryLog,
  type RootLog,
  type ScopeLog,
  type SkippedOptionalDependencyLog,
  type StageLog,
  type StatsLog,
  type SummaryLog,
  type UpdateCheckLog,
} from './all'

export * from './all'

export type Log =
  | ContextLog
  | DeprecationLog
  | FetchingProgressLog
  | ExecutionTimeLog
  | HookLog
  | InstallCheckLog
  | IgnoredScriptsLog
  | LifecycleLog
  | LinkLog
  | PackageManifestLog
  | PackageImportMethodLog
  | PeerDependencyIssuesLog
  | ProgressLog
  | RegistryLog
  | RequestRetryLog
  | RootLog
  | ScopeLog
  | SkippedOptionalDependencyLog
  | StageLog
  | StatsLog
  | SummaryLog
  | UpdateCheckLog
