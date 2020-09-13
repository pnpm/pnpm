import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const progressLogger = baseLogger('progress') as Logger<ProgressMessage> // eslint-disable-line

export type ProgressMessage = {
  packageId: string
  requester: string
  status: 'fetched' | 'found_in_store' | 'resolved'
} | {
  status: 'imported'
  method: string
  requester: string
  to: string
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage
