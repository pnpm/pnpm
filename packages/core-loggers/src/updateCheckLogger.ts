import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const updateCheckLogger = baseLogger('update-check')

export interface UpdateCheckMessage {
  currentVersion: string
  latestVersion: string
}

export type UpdateCheckLog = {name: 'pnpm:update-check'} & LogBase & UpdateCheckMessage
