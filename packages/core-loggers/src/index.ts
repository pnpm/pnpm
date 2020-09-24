import {
  ContextLog,
  DeprecationLog,
  FetchingProgressLog,
  HookLog,
  InstallCheckLog,
  LifecycleLog,
  LinkLog,
  PackageImportMethodLog,
  PackageManifestLog,
  ProgressLog,
  RegistryLog,
  RequestRetryLog,
  RootLog,
  ScopeLog,
  SkippedOptionalDependencyLog,
  StageLog,
  StatsLog,
  SummaryLog,
} from './all'

export * from './all'

export type Log =
  | ContextLog
  | DeprecationLog
  | FetchingProgressLog
  | HookLog
  | InstallCheckLog
  | LifecycleLog
  | LinkLog
  | PackageManifestLog
  | PackageImportMethodLog
  | ProgressLog
  | RegistryLog
  | RequestRetryLog
  | RootLog
  | ScopeLog
  | SkippedOptionalDependencyLog
  | StageLog
  | StatsLog
  | SummaryLog
