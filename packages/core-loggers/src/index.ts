export * from './all'

import {
  DeprecationLog,
  FetchingProgressLog,
  HookLog,
  ImportingLog,
  InstallCheckLog,
  LifecycleLog,
  LinkLog,
  PackageManifestLog,
  ProgressLog,
  RegistryLog,
  RootLog,
  ScopeLog,
  SkippedOptionalDependencyLog,
  StageLog,
  StatsLog,
  SummaryLog,
} from './all'

export type Log = DeprecationLog
  | FetchingProgressLog
  | HookLog
  | ImportingLog
  | InstallCheckLog
  | LifecycleLog
  | LinkLog
  | PackageManifestLog
  | ProgressLog
  | RegistryLog
  | RootLog
  | ScopeLog
  | SkippedOptionalDependencyLog
  | StageLog
  | StatsLog
  | SummaryLog
