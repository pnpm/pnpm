import {
  LogBase,
  Logger,
  logger,
} from '@pnpm/logger'

export const progressLogger = logger('progress') as Logger<ProgressMessage>

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

export type ProgressLog = { name: 'pnpm:progress' } & LogBase & ProgressMessage
