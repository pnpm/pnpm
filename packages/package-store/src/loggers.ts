import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const importingLogger = baseLogger('importing') as Logger<ImportingMessage>

export interface ImportingMessage {
  from: string,
  method: string,
  to: string,
}

export type ImportingLog = {name: 'pnpm:importing'} & LogBase & ImportingMessage
