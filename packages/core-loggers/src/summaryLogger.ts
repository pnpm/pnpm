import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const summaryLogger = logger<SummaryMessage>('summary')

export interface SummaryMessage {
  prefix: string
}

export type SummaryLog = { name: 'pnpm:summary' } & LogBase & SummaryMessage
