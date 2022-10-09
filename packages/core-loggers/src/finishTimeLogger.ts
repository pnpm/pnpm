import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const finishTimeLogger = baseLogger('finish-time')

export interface FinishTimeMessage {
  startedAt: number
  finishedAt: number
}

export type FinishTimeLog = { name: 'pnpm:finish-time' } & LogBase & FinishTimeMessage
