import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const ignoredScriptsLogger = logger('ignored-scripts')

export interface IgnoredScriptsMessage {
  packageNames: string[]
}

export type IgnoredScriptsLog = { name: 'pnpm:ignored-scripts' } & LogBase & IgnoredScriptsMessage
