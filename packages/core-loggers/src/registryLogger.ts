import {
  LogBase,
} from '@pnpm/logger'

export type RegistryLog = { name: 'pnpm:registry' } & LogBase & { message: string }
