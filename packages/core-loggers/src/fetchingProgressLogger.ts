import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const fetchingProgressLogger = baseLogger('fetching-progress') as Logger<FetchingProgressMessage> // eslint-disable-line

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

export type FetchingProgressLog = {name: 'pnpm:fetching-progress'} & LogBase & FetchingProgressMessage
