import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'

export const packageJsonLogger = baseLogger('package-json') as Logger<PackageJsonMessage>
export const stageLogger = baseLogger('stage') as Logger<'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done'>
export const summaryLogger = baseLogger('summary') as Logger<SummaryMessage>
export const rootLogger = baseLogger('root') as Logger<RootMessage>
export const statsLogger = baseLogger('stats') as Logger<StatsMessage>
export const skippedOptionalDependencyLogger = baseLogger('skipped-optional-dependency') as Logger<SkippedOptionalDependencyMessage>
export const progressLogger = baseLogger('progress') as Logger<ProgressMessage>

export type PackageJsonMessage = {
  prefix: string,
} & ({
  initial: PackageJson,
} | {
  updated: object,
})

export type PackageJsonLog = {name: 'pnpm:package-json'} & LogBase & PackageJsonMessage

export type DependencyType = 'prod' | 'dev' | 'optional'

export type RootMessage = {
  prefix: string,
} & ({
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
})

export type RootLog = {name: 'pnpm:root'} & LogBase & RootMessage

export type StatsMessage = {
  prefix: string,
} & ({
  added: number,
} | {
  removed: number,
})

export type SkippedOptionalDependencyMessage = {
  details?: string,
  parents?: Array<{id: string, name: string, version: string}>,
} & ({
  package: {
    id: string,
    name: string,
    version: string,
  },
  reason: 'unsupported_engine'
    | 'unsupported_platform'
    | 'build_failure',
} | {
  package: {
    name: string | undefined,
    version: string | undefined,
    pref: string,
  },
  reason: 'resolution_failure',
})

export type StatsLog = {name: 'pnpm:stats'} & LogBase & StatsMessage

export type SkippedOptionalDependencyLog = {name: 'pnpm:skipped-optional-dependency'} & LogBase & SkippedOptionalDependencyMessage

export type StageLog = {name: 'pnpm:stage'} & LogBase & {message: 'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done'}

export interface SummaryMessage {
  prefix: string,
}

export type SummaryLog = {name: 'pnpm:summary'} & LogBase & SummaryMessage

export interface LoggedPkg {
  rawSpec: string,
  name?: string, // sometimes known for the root dependency on named installation
  dependentId?: string,
}

export type ProgressMessage = {
  pkg: LoggedPkg,
  status: 'installing',
} | {
 status: 'downloaded_manifest',
 pkgId: string,
 pkgVersion: string,
} | {
 pkgId: string,
 status: 'fetched' | 'found_in_store' | 'resolving_content',
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage

export type Log = StageLog
  | StatsLog
  | SkippedOptionalDependencyLog
  | RootLog
  | PackageJsonLog
  | SummaryLog
  | ProgressLog
