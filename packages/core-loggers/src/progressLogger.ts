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
  pkgId: string,
  pkg: LoggedPkg,
  status: 'resolved',
} | {
  pkg: LoggedPkg,
  status: 'error',
} | {
  pkgId: string,
  status: 'fetching_started',
  size: number | null,
  attempt: number,
} | {
  pkgId: string,
  status: 'fetching_progress',
  downloaded: number,
} | {
  pkg: LoggedPkg,
  status: 'installing',
} | {
  status: 'downloaded_manifest',
  pkgId: string,
  pkgVersion: string,
} | {
  pkgId: string,
  status: 'fetched' | 'found_in_store' | 'resolving_content',
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage
