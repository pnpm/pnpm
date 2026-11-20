import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const requestRetryLogger = logger<RequestRetryMessage>('request-retry')

export interface RequestRetryError {
  name?: string
  message?: string
  // HTTP status codes (numeric)
  status?: number
  statusCode?: number
  // System error properties
  errno?: number
  code?: string
  // undici wraps the actual error in a cause property
  cause?: {
    code?: string
    errno?: number
  }
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
