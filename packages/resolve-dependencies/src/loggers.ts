import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const installCheckLogger = baseLogger('install-check') as Logger<InstallCheckMessage> // tslint:disable-line
export const deprecationLogger = baseLogger('deprecation') as Logger<DeprecationMessage> // tslint:disable-line

export interface InstallCheckMessage {
  code: string,
  pkgId: string,
}

export type InstallCheckLog = {name: 'pnpm:install-check'} & LogBase & InstallCheckMessage

export interface DeprecationMessage {
  pkgName: string,
  pkgVersion: string,
  pkgId: string,
  prefix: string,
  deprecated: string,
  depth: number,
}

export type DeprecationLog = {name: 'pnpm:deprecation'} & LogBase & DeprecationMessage

export type Log = DeprecationLog | InstallCheckLog
