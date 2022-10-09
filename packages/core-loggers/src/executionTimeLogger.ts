import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const executionTimeLogger = baseLogger('execution-time')

export interface ExecutionTimeMessage {
  startedAt: number
  endedAt: number
}

export type ExecutionTimeLog = { name: 'pnpm:execution-time' } & LogBase & ExecutionTimeMessage
