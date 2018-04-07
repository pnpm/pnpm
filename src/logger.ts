import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'

export const lifecycleLogger = baseLogger('lifecycle') as Logger<LifecycleMessage>

export type LifecycleMessage = {
  depPath: string,
  stage: string,
} & ({
  line: string,
} | {
  exitCode: number,
} | {
  script: string,
})

export type LifecycleLog = {name: 'pnpm:lifecycle'} & LogBase & LifecycleMessage
