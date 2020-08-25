import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const statsLogger = baseLogger<StatsMessage>('stats')

export type StatsMessage = {
  prefix: string
} & ({
  added: number
} | {
  removed: number
})

export type StatsLog = {name: 'pnpm:stats'} & LogBase & StatsMessage
