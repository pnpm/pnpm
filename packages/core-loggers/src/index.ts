import type {
  ContextLog,
  DeprecationLog,
  FetchingProgressLog,
  ExecutionTimeLog,
  HookLog,
  InstallCheckLog,
  InstallingConfigDepsLog,
  IgnoredScriptsLog,
  LifecycleLog,
  LinkLog,
  PackageImportMethodLog,
  PackageManifestLog,
  PeerDependencyIssuesLog,
  ProgressLog,
  RegistryLog,
  RequestRetryLog,
  RootLog,
  ScopeLog,
  SkippedOptionalDependencyLog,
  StageLog,
  StatsLog,
  SummaryLog,
  UpdateCheckLog,
} from './all.js'

export * from './all.js'

export type Log =
  | ContextLog
  | DeprecationLog
  | FetchingProgressLog
  | ExecutionTimeLog
  | HookLog
  | InstallCheckLog
  | InstallingConfigDepsLog
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
