// Patch the global fs module here at the app level
import './fs/gracefulify'

export {PackageManifest, PnpmOptions} from '@pnpm/types'
export * from './api'
export {PnpmError, PnpmErrorCode} from './errorTypes'
export {
  PackageJsonLog,
  InstallCheckLog,
  DeprecationLog,
  LifecycleLog,
  RootLog,
  StageLog,
  RegistryLog,
} from './loggers'

import * as supiLogs from './loggers'
import * as packageRequesterLogs from '@pnpm/package-requester'

export type ProgressLog = supiLogs.ProgressLog | packageRequesterLogs.ProgressLog
export type Log = supiLogs.Log | packageRequesterLogs.Log
