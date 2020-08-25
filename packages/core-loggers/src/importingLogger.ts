import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const importingLogger = baseLogger('importing')

export interface ImportingMessage {
  from: string
  method: string
  to: string
}

export type ImportingLog = {name: 'pnpm:importing'} & LogBase & ImportingMessage
