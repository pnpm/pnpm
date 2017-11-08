import baseLogger, {
  LogBase,
  Logger,
} from 'pnpm-logger'

export const progressLogger = baseLogger('progress') as Logger<ProgressMessage>

export interface LoggedPkg {
  rawSpec: string,
  name: string,
  dependentId?: string,
}

// Not all of this message types are used in this project
// some of them can be removed
export type ProgressMessage = {
  pkgId: string,
  status: 'fetched' | 'installed' | 'dependencies_installed' | 'found_in_store' | 'resolving_content',
} | {
  pkgId: string,
  pkg: LoggedPkg,
  status: 'resolved',
} | {
  pkg: LoggedPkg,
  status: 'resolving' | 'error' | 'installing',
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
  status: 'downloaded_manifest',
  pkgId: string,
  pkgVersion: string,
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage

export type Log = ProgressLog
