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
import {
  DeprecationLog,
  InstallCheckLog,
} from '@pnpm/resolve-dependencies'
export * from './api'
export { PnpmError, PnpmErrorCode } from './errorTypes'
export {
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
  InstallCheckLog,
  DeprecationLog,
}

export { InstallOptions } from './install/extendInstallOptions'
export { RebuildOptions } from './rebuild/extendRebuildOptions'
export { UninstallOptions } from './uninstall/extendUninstallOptions'

import * as packageRequesterLogs from '@pnpm/package-requester'
import * as supiLogs from './loggers'

export { LocalPackages } from '@pnpm/resolver-base'

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
  | InstallCheckLog
  | DeprecationLog
