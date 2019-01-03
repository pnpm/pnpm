import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const stageLogger = baseLogger<StageMessage>('stage')

export type StageMessage = {
  prefix: string,
  stage: 'resolution_started'
    | 'resolution_done'
    | 'importing_started'
    | 'importing_done',
}

export type StageLog = {name: 'pnpm:stage'} & LogBase & StageMessage
