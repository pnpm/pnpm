import { type RetryTimeoutOptions } from '@zkochan/retry'

export type { RetryTimeoutOptions }

export interface RequestInit extends globalThis.RequestInit {
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
