import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const contextLogger = baseLogger('context')

export interface ContextMessage {
  currentLockfileExists: boolean
  storeDir: string
  virtualStoreDir: string
}

export type ContextLog = {name: 'pnpm:context'} & LogBase & ContextMessage
