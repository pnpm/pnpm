// Patch the global fs module here at the app level
import './fs/gracefulify'

export * from './api'
export {PnpmOptions, PackageManifest} from './types'
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
import * as packageStoreLogs from 'package-store'

export type ProgressLog = supiLogs.ProgressLog | packageStoreLogs.ProgressLog
export type Log = supiLogs.Log | packageStoreLogs.Log
