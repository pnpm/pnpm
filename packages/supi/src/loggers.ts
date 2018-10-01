import { LogBase } from '@pnpm/logger'

export interface InstallCheckMessage {
  code: string,
  pkgId: string,
}

export type RegistryLog = {name: 'pnpm:registry'} & LogBase & {message: string}

export type Log = RegistryLog
