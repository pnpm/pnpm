import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'
import { LoggedPkg } from '@pnpm/store-controller-types'

export const progressLogger = baseLogger('progress') as Logger<ProgressMessage> // tslint:disable-line

// Not all of this message types are used in this project
// some of them can be removed
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
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage

export type Log = ProgressLog
