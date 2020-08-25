import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const skippedOptionalDependencyLogger = baseLogger<SkippedOptionalDependencyMessage>('skipped-optional-dependency')

export type SkippedOptionalDependencyLog = {name: 'pnpm:skipped-optional-dependency'} & LogBase & SkippedOptionalDependencyMessage

export type SkippedOptionalDependencyMessage = {
  details?: string
  parents?: Array<{id: string, name: string, version: string}>
  prefix: string
} & ({
  package: {
    id: string
    name: string
    version: string
  }
  reason: 'unsupported_engine'
  | 'unsupported_platform'
  | 'build_failure'
} | {
  package: {
    name: string | undefined
    version: string | undefined
    pref: string
  }
  reason: 'resolution_failure'
})
