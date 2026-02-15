import type { RetryTimeoutOptions } from '@zkochan/retry'
import type { Response, RequestInit as NodeRequestInit } from 'node-fetch'

export type { RetryTimeoutOptions, Response }

export interface RequestInit extends NodeRequestInit {
  retry?: RetryTimeoutOptions
  timeout?: number
}

export type FetchFromRegistry = (
  url: string,
  opts?: RequestInit & {
    authHeaderValue?: string
    compress?: boolean
    fullMetadata?: boolean
    retry?: RetryTimeoutOptions
    timeout?: number
  }
) => Promise<Response>

export type GetAuthHeader = (uri: string) => string | undefined
