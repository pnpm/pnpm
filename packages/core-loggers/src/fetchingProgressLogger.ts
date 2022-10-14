import {
  LogBase,
  Logger,
  logger,
} from '@pnpm/logger'

export const fetchingProgressLogger = logger('fetching-progress') as Logger<FetchingProgressMessage>

export type FetchingProgressMessage = {
  attempt: number
  packageId: string
  size: number | null
  status: 'started'
} | {
  downloaded: number
  packageId: string
  status: 'in_progress'
}

export type FetchingProgressLog = { name: 'pnpm:fetching-progress' } & LogBase & FetchingProgressMessage
