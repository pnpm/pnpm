import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const statsLogger = logger<StatsMessage>('stats')

export type StatsMessage = {
  prefix: string
} & ({
  added: number
} | {
  removed: number
})

export type StatsLog = { name: 'pnpm:stats' } & LogBase & StatsMessage
