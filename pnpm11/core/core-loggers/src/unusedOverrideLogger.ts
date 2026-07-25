import {
  type LogBase,
  type Logger,
  logger,
} from '@pnpm/logger'

export const unusedOverrideLogger = logger('unused-override') as Logger<UnusedOverrideMessage>

export interface UnusedOverrideMessage {
  prefix: string
  selector: string
}

export type UnusedOverrideLog = { name: 'pnpm:unused-override' } & LogBase & UnusedOverrideMessage
