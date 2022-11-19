import { RetryTimeoutOptions } from '@zkochan/retry'
import { Response } from 'node-fetch'

export { RetryTimeoutOptions }

export type FetchFromRegistry = (
  url: string,
  opts?: {
    authHeaderValue?: string
    compress?: boolean
    retry?: RetryTimeoutOptions
    timeout?: number
  }
) => Promise<Response>

export type GetAuthHeader = (uri: string) => string | undefined
