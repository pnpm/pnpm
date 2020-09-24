import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const summaryLogger = baseLogger<SummaryMessage>('summary')

export interface SummaryMessage {
  prefix: string
}

export type SummaryLog = {name: 'pnpm:summary'} & LogBase & SummaryMessage
