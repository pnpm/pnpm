import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const requestRetryLogger = baseLogger<RequestRetryMessage>('request-retry')

export interface RequestRetryMessage {
  attempt: number
  error: Error
  maxRetries: number
  method: string
  timeout: number
  url: string
}

export type RequestRetryLog = { name: 'pnpm:request-retry' } & LogBase & RequestRetryMessage
