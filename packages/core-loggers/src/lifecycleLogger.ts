import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const lifecycleLogger = logger<LifecycleMessage>('lifecycle')

// TODO: make depPath optional
export interface LifecycleMessageBase {
  depPath: string
  stage: string
  wd: string
  exitCode?: number
  line?: string
  optional?: boolean
  script?: string
  stdio?: 'stdout' | 'stderr'
}

export interface StdioLifecycleMessage extends LifecycleMessageBase {
  line: string
  stdio: 'stdout' | 'stderr'
}

export interface ExitLifecycleMessage extends LifecycleMessageBase {
  exitCode: number
  optional: boolean
}

export interface ScriptLifecycleMessage extends LifecycleMessageBase {
  script: string
  optional: boolean
}

export type LifecycleMessage = StdioLifecycleMessage | ExitLifecycleMessage | ScriptLifecycleMessage

export type LifecycleLog = { name: 'pnpm:lifecycle' } & LogBase & LifecycleMessage
