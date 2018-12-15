import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const rootLogger = baseLogger<RootMessage>('root')

export type DependencyType = 'prod' | 'dev' | 'optional'

export type RootMessage = {
  prefix: string,
} & ({
  added: {
    name: string,
    realName: string,
    version?: string,
    dependencyType?: DependencyType,
    linkedFrom?: string,
  },
} | {
  removed: {
    name: string,
    version?: string,
    dependencyType?: DependencyType,
  },
})

export type RootLog = {name: 'pnpm:root'} & LogBase & RootMessage
