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
  StatsLog,
} from './loggers'

export {InstallOptions} from './api/extendInstallOptions'
export {PruneOptions} from './api/extendPruneOptions'
export {RebuildOptions} from './api/extendRebuildOptions'
export {UninstallOptions} from './api/extendUninstallOptions'

import * as packageRequesterLogs from '@pnpm/package-requester'
import * as supiLogs from './loggers'

export type ProgressLog = supiLogs.ProgressLog | packageRequesterLogs.ProgressLog
export type Log = supiLogs.Log | packageRequesterLogs.Log
