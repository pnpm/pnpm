import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const installCheckLogger = logger<InstallCheckMessage>('install-check')

export interface InstallCheckMessage {
  code: string
  pkgId: string
}

export type InstallCheckLog = { name: 'pnpm:install-check' } & LogBase & InstallCheckMessage
