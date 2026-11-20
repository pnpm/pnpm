import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const installingConfigDepsLogger = logger<InstallingConfigDepsMessage>('installing-config-deps')

export interface InstallingConfigDepsMessageBase {
  status?: 'started' | 'done'
}

export interface InstallingConfigDepsStartedMessage extends InstallingConfigDepsMessageBase {
  status: 'started'
}

export interface InstallingConfigDepsDoneMessage extends InstallingConfigDepsMessageBase {
  deps: Array<{ name: string, version: string }>
  status: 'done'
}

export type InstallingConfigDepsMessage = InstallingConfigDepsStartedMessage | InstallingConfigDepsDoneMessage

export type InstallingConfigDepsLog = { name: 'pnpm:installing-config-deps' } & LogBase & InstallingConfigDepsMessage
