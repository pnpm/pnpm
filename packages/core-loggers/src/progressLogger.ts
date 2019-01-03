import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const progressLogger = baseLogger('progress') as Logger<ProgressMessage> // tslint:disable-line

export type ProgressMessage = {
  packageId: string,
  requester: string,
  status: 'fetched' | 'found_in_store' | 'resolved',
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage
