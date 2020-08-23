import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const progressLogger = baseLogger('progress') as Logger<ProgressMessage> // eslint-disable-line

export interface ProgressMessage {
  packageId: string,
  requester: string,
  status: 'fetched' | 'found_in_store' | 'resolved',
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage
