import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const executionTimeLogger = logger('execution-time')

export interface ExecutionTimeMessage {
  startedAt: number
  endedAt: number
}

export type ExecutionTimeLog = { name: 'pnpm:execution-time' } & LogBase & ExecutionTimeMessage
