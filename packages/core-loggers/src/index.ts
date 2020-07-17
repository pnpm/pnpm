export * from './all'

import {
  DeprecationLog,
  FetchingProgressLog,
  HookLog,
  ImportingLog,
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

export type Log =
  | DeprecationLog
  | FetchingProgressLog
  | HookLog
  | ImportingLog
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
