import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const linkLogger = baseLogger<LinkMessage>('link')

export interface LinkMessage {
  target: string
  link: string
}

export type LinkLog = {name: 'pnpm:link'} & LogBase & LinkMessage
