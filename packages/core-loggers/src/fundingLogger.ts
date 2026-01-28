import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const fundingLogger = logger<FundingMessage>('funding')

export type FundingType = 'funding' | 'repository' | 'homepage'

export interface FundingMessage {
  packageName: string
  packageDescription?: string
  fundingUrl: string
  fundingType: FundingType
}

export type FundingLog = { name: 'pnpm:funding' } & LogBase & FundingMessage
