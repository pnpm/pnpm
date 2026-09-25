import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const statsLogger = logger<StatsMessage>('stats')

export interface StatsMessageBase {
  prefix: string
  added?: number
  removed?: number
}

export interface StatsMessageAdded extends StatsMessageBase {
  added: number
  removed?: never
}

export interface StatsMessageRemoved extends StatsMessageBase {
  added?: never
  removed: number
}

export type StatsMessage = StatsMessageAdded | StatsMessageRemoved

export type StatsLog = { name: 'pnpm:stats' } & LogBase & StatsMessage
