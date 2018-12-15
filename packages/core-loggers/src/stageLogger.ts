import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const stageLogger = baseLogger<'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done' | 'recursive_importing_done'>('stage')

export type StageLog = {name: 'pnpm:stage'} & LogBase & {message: 'resolution_started' | 'resolution_done' | 'importing_started' | 'importing_done'}
