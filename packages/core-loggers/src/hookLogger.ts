import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const hookLogger = logger('hook')

export interface HookMessage {
  from: string
  hook: string
  message: string
  prefix: string
}

export type HookLog = { name: 'pnpm:hook' } & LogBase & HookMessage
