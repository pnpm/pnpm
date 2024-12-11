import { type RetryTimeoutOptions } from '@zkochan/retry'
import { type Response, type RequestInit as NodeRequestInit } from 'node-fetch'

export type { RetryTimeoutOptions }

export interface RequestInit extends NodeRequestInit {
  retry?: RetryTimeoutOptions
  timeout?: number
}

export type FetchFromRegistry = (
  url: string,
  opts?: RequestInit & {
    authHeaderValue?: string
    compress?: boolean
    retry?: RetryTimeoutOptions
    timeout?: number
  }
) => Promise<Response>

export type GetAuthHeader = (uri: string) => string | undefined
