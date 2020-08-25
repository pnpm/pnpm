import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const scopeLogger = baseLogger<ScopeMessage>('scope')

export interface ScopeMessage {
  selected: number
  total?: number
  workspacePrefix?: string
}

export type ScopeLog = {name: 'pnpm:scope'} & LogBase & ScopeMessage
