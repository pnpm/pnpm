import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const hookLogger = baseLogger('hook')

export interface HookMessage {
  from: string
  hook: string
  message: string
  prefix: string
}

export type HookLog = {name: 'pnpm:hook'} & LogBase & HookMessage
