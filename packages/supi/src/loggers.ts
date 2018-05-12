import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const installCheckLogger = baseLogger('install-check') as Logger<InstallCheckMessage>
export const deprecationLogger = baseLogger('deprecation') as Logger<DeprecationMessage>
export const progressLogger = baseLogger('progress') as Logger<ProgressMessage>

export interface InstallCheckMessage {
  code: string,
  pkgId: string,
}

export type InstallCheckLog = {name: 'pnpm:install-check'} & LogBase & InstallCheckMessage

export interface DeprecationMessage {
  pkgName: string,
  pkgVersion: string,
  pkgId: string,
  deprecated: string,
  depth: number,
}

export type DeprecationLog = {name: 'pnpm:deprecation'} & LogBase & DeprecationMessage

export interface LoggedPkg {
  rawSpec: string,
  name?: string, // sometimes known for the root dependency on named installation
  dependentId?: string,
}

export type ProgressMessage = {
   pkg: LoggedPkg,
   status: 'installing',
} | {
  status: 'downloaded_manifest',
  pkgId: string,
  pkgVersion: string,
}

export type ProgressLog = {name: 'pnpm:progress'} & LogBase & ProgressMessage

export type RegistryLog = {name: 'pnpm:registry'} & LogBase & {message: string}

export type Log = ProgressLog
  | DeprecationLog
  | InstallCheckLog
  | RegistryLog
