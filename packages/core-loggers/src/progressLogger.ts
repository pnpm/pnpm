import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const progressLogger = baseLogger('progress') as Logger<ProgressMessage> // tslint:disable-line

export interface LoggedPkg {
  rawSpec: string,
  name?: string, // sometimes known for the root dependency on named installation
  dependentId?: string,
}

export type ProgressMessage = {
  packageId: string,
  context: string,
  status: 'fetched' | 'found_in_store' | 'resolving_content',
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage
