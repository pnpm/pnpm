import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const linkLogger = logger<LinkMessage>('link')

export interface LinkMessage {
  target: string
  link: string
}

export type LinkLog = { name: 'pnpm:link' } & LogBase & LinkMessage
