import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'

export const packageJsonLogger = baseLogger('package-json') as Logger<PackageJsonMessage>
export const stageLogger = baseLogger('stage') as Logger<'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done'>
export const summaryLogger = baseLogger('summary') as Logger<void>
export const rootLogger = baseLogger('root') as Logger<RootMessage>
export const statsLogger = baseLogger('stats') as Logger<StatsMessage>
export const skippedOptionalDependencyLogger = baseLogger('skipped-optional-dependency') as Logger<SkippedOptionalDependencyMessage>

export type PackageJsonMessage = {
  initial: PackageJson,
} | {
  updated: object,
}

export type PackageJsonLog = {name: 'pnpm:package-json'} & LogBase & PackageJsonMessage

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

export type SummaryLog = {name: 'pnpm:summary'} & LogBase

export type Log = StageLog
  | StatsLog
  | SkippedOptionalDependencyLog
  | RootLog
  | PackageJsonLog
  | SummaryLog
