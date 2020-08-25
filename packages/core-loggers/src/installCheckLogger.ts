import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const installCheckLogger = baseLogger<InstallCheckMessage>('install-check')

export interface InstallCheckMessage {
  code: string
  pkgId: string
}

export type InstallCheckLog = {name: 'pnpm:install-check'} & LogBase & InstallCheckMessage
