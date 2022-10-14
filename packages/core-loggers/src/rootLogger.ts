import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const rootLogger = logger<RootMessage>('root')

export type DependencyType = 'prod' | 'dev' | 'optional'

export type RootMessage = {
  prefix: string
} & ({
  added: {
    id?: string
    name: string
    realName: string
    version?: string
    dependencyType?: DependencyType
    latest?: string
    linkedFrom?: string
  }
} | {
  removed: {
    name: string
    version?: string
    dependencyType?: DependencyType
  }
})

export type RootLog = { name: 'pnpm:root' } & LogBase & RootMessage
