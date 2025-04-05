import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const installedConfigDepsLogger = logger<InstalledConfigDepsMessage>('installed-config-deps')

export interface InstalledConfigDepsMessage {
  deps: Array<{ name: string, version: string }>
}

export type InstalledConfigDepsLog = { name: 'pnpm:installed-config-deps' } & LogBase & InstalledConfigDepsMessage
