import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const updateCheckLogger = logger('update-check')

export interface UpdateCheckMessage {
  currentVersion: string
  latestVersion: string
}

export type UpdateCheckLog = { name: 'pnpm:update-check' } & LogBase & UpdateCheckMessage
