import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const requestRetryLogger = logger<RequestRetryMessage>('request-retry')

export interface RequestRetryError extends Error {
  httpStatusCode?: string
  status?: string
  errno?: number
  code?: string
}

export interface RequestRetryMessage {
  attempt: number
  error: RequestRetryError
  maxRetries: number
  method: string
  timeout: number
  url: string
}

export type RequestRetryLog = { name: 'pnpm:request-retry' } & LogBase & RequestRetryMessage
