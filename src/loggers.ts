import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'

export const packageJsonLogger = baseLogger('package-json') as Logger<PackageJsonMessage>
export const stageLogger = baseLogger('stage') as Logger<'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done'>
export const summaryLogger = baseLogger('summary') as Logger<void>
export const installCheckLogger = baseLogger('install-check') as Logger<InstallCheckMessage>
export const deprecationLogger = baseLogger('deprecation') as Logger<DeprecationMessage>
export const lifecycleLogger = baseLogger('lifecycle') as Logger<LifecycleMessage>
export const rootLogger = baseLogger('root') as Logger<RootMessage>
export const progressLogger = baseLogger('progress') as Logger<ProgressMessage>
export const statsLogger = baseLogger('stats') as Logger<StatsMessage>

export type PackageJsonMessage = {
  initial: PackageJson,
} | {
  updated: object,
}

export type PackageJsonLog = {name: 'pnpm:package-json'} & LogBase & PackageJsonMessage

export interface InstallCheckMessage {
  code: string,
  pkgId: string,
}

export type InstallCheckLog = {name: 'pnpm:install-check'} & LogBase & InstallCheckMessage

export interface DeprecationMessage {
  pkgName: string,
  pkgVersion: string,
  pkgId: string,
  deprecated: string,
  depth: number,
}

export type DeprecationLog = {name: 'pnpm:deprecation'} & LogBase & DeprecationMessage

export type LifecycleMessage = {
  pkgId: string,
  script: string,
} & ({
  line: string,
} | {
  exitCode: number,
})

export type LifecycleLog = {name: 'pnpm:lifecycle'} & LogBase & LifecycleMessage

export type DependencyType = 'prod' | 'dev' | 'optional'

export type RootMessage = {
  added: {
    name: string,
    realName: string,
    version: string,
    dependencyType: DependencyType,
  },
} | {
  removed: {
    name: string,
    version?: string,
    dependencyType: DependencyType,
  },
} | {
  linked: {
    name: string,
    from: string,
    to: string,
    dependencyType?: DependencyType,
  },
}

export type RootLog = {name: 'pnpm:root'} & LogBase & RootMessage

export interface LoggedPkg {
  rawSpec: string,
  name?: string, // sometimes known for the root dependency on named installation
  dependentId?: string,
}

export type ProgressMessage = {
  pkgId: string,
  status: 'installed' | 'dependencies_installed',
} | {
   pkg: LoggedPkg,
   status: 'installing',
} | {
  status: 'downloaded_manifest',
  pkgId: string,
  pkgVersion: string,
}

export type StatsMessage = {
  added: number,
} | {
  removed: number,
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage

export type StageLog = {name: 'pnpm:stage'} & LogBase & {message: 'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done'}

export type StatsLog = {name: 'pnpm:stats'} & LogBase & StatsMessage

export type RegistryLog = {name: 'pnpm:registry'} & LogBase & {message: string}

export type Log = StageLog
  | ProgressLog
  | RootLog
  | LifecycleLog
  | DeprecationLog
  | InstallCheckLog
  | PackageJsonLog
  | RegistryLog
  | StatsLog
  | {name: 'pnpm:summary'} & LogBase
