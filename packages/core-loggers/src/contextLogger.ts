import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const contextLogger = logger('context')

export interface ContextMessage {
  currentLockfileExists: boolean
  storeDir: string
  virtualStoreDir: string
}

export type ContextLog = { name: 'pnpm:context' } & LogBase & ContextMessage
