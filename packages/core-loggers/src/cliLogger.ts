import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const cliLogger = baseLogger<'command_done'>('cli')

export type CliLog = {name: 'pnpm:cli'} & LogBase & {message: 'command_done'}
