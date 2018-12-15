export * from './all'

import {
  CliLog,
  DeprecationLog,
  HookLog,
  ImportingLog,
  InstallCheckLog,
  LifecycleLog,
  LinkLog,
  PackageJsonLog,
  ProgressLog,
  RegistryLog,
  RootLog,
  ScopeLog,
  SkippedOptionalDependencyLog,
  StageLog,
  StatsLog,
  SummaryLog,
} from './all'

export type Log = CliLog
  | DeprecationLog
  | HookLog
  | ImportingLog
  | InstallCheckLog
  | LifecycleLog
  | LinkLog
  | PackageJsonLog
  | ProgressLog
  | RegistryLog
  | RootLog
  | ScopeLog
  | SkippedOptionalDependencyLog
  | StageLog
  | StatsLog
  | SummaryLog
