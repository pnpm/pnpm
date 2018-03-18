import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const lifecycleLogger = baseLogger('lifecycle') as Logger<LifecycleMessage>

export type LifecycleMessage = {
  pkgId: string,
  script: string,
} & ({
  line: string,
} | {
  exitCode: number,
})

export type LifecycleLog = {name: 'pnpm:lifecycle'} & LogBase & LifecycleMessage
