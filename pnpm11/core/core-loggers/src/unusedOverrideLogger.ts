import {
  type LogBase,
  type Logger,
  logger,
} from '@pnpm/logger'

export const unusedOverrideLogger = logger('unusedOverride') as Logger<UnusedOverrideMessage>

export interface UnusedOverrideMessage {
  prefix: string
  selector: string
}

export type UnusedOverrideLog = { name: 'pnpm:unusedOverride' } & LogBase & UnusedOverrideMessage
