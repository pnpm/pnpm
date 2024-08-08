import {
  type LogBase,
  type Logger,
  logger,
} from '@pnpm/logger'

export const fetchingProgressLogger = logger('fetching-progress') as Logger<FetchingProgressMessage>

export interface FetchingProgressMessageBase {
  attempt?: number
  downloaded?: number
  packageId: string
  size?: number | null
  status?: 'started' | 'in_progress'
}

export interface FetchingProgressMessageStarted extends FetchingProgressMessageBase {
  attempt: number
  size: number | null
  status: 'started'
}

export interface FetchingProgressMessageInProgress extends FetchingProgressMessageBase {
  downloaded: number
  status: 'in_progress'
}

export type FetchingProgressMessage = FetchingProgressMessageStarted | FetchingProgressMessageInProgress

export type FetchingProgressLog = { name: 'pnpm:fetching-progress' } & LogBase & FetchingProgressMessage
