import '@total-typescript/ts-reset'
import type { RetryTimeoutOptions } from '@zkochan/retry'
import type { Response } from 'node-fetch'

export type { RetryTimeoutOptions }

export type FetchFromRegistry = (
  url: string,
  opts?: {
    authHeaderValue?: string | undefined
    compress?: boolean | undefined
    retry?: RetryTimeoutOptions | undefined
    timeout?: number | undefined
  } | undefined
) => Promise<Response>

export type GetAuthHeader = (uri: string) => string | undefined
