import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const deprecationLogger = baseLogger('deprecation') as Logger<DeprecationMessage> // eslint-disable-line

export interface DeprecationMessage {
  pkgName: string
  pkgVersion: string
  pkgId: string
  prefix: string
  deprecated: string
  depth: number
}

export type DeprecationLog = {name: 'pnpm:deprecation'} & LogBase & DeprecationMessage
