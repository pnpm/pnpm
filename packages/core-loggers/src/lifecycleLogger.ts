import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const lifecycleLogger = logger<LifecycleMessage>('lifecycle')

// TODO: make depPath optional
export type LifecycleMessage = {
  depPath: string
  stage: string
  wd: string
} & ({
  line: string
  stdio: 'stdout' | 'stderr'
} | {
  exitCode: number
  optional: boolean
} | {
  script: string
  optional: boolean
})

export type LifecycleLog = { name: 'pnpm:lifecycle' } & LogBase & LifecycleMessage
