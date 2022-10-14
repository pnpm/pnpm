import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const scopeLogger = logger<ScopeMessage>('scope')

export interface ScopeMessage {
  selected: number
  total?: number
  workspacePrefix?: string
}

export type ScopeLog = { name: 'pnpm:scope' } & LogBase & ScopeMessage
