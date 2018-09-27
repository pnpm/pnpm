// Patch the global fs module here at the app level
import './fs/gracefulify'

import {
  PackageJsonLog,
  ProgressLog,
  RootLog,
  SkippedOptionalDependencyLog,
  StageLog,
  StatsLog,
  SummaryLog,
} from '@pnpm/core-loggers'
export { PackageManifest, PnpmOptions } from '@pnpm/types'
export * from './api'
export { PnpmError, PnpmErrorCode } from './errorTypes'
export {
  InstallCheckLog,
  DeprecationLog,
  RegistryLog,
} from './loggers'
import { LifecycleLog } from '@pnpm/lifecycle'

export {
  LifecycleLog,
  RootLog,
  StatsLog,
  SkippedOptionalDependencyLog,
  StageLog,
  PackageJsonLog,
  SummaryLog,
}

export { InstallOptions } from './api/extendInstallOptions'
export { RebuildOptions } from './api/extendRebuildOptions'
export { UninstallOptions } from './api/extendUninstallOptions'

import * as packageRequesterLogs from '@pnpm/package-requester'
import * as supiLogs from './loggers'

export type ProgressLog = ProgressLog | packageRequesterLogs.ProgressLog
export type Log = supiLogs.Log
  | packageRequesterLogs.Log
  | LifecycleLog
  | RootLog
  | StatsLog
  | SkippedOptionalDependencyLog
  | PackageJsonLog
  | StageLog
  | SummaryLog
  | ProgressLog
