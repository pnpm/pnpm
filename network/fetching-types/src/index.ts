import { type RetryTimeoutOptions } from '@zkochan/retry'
import { type Response } from 'node-fetch'

export type { RetryTimeoutOptions }

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
