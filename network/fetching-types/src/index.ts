import { type RetryTimeoutOptions } from '@zkochan/retry'
import { type Response, type RequestInit as NodeRequestInit } from 'node-fetch'

export type { RetryTimeoutOptions }

export interface RequestInit extends NodeRequestInit {
  retry?: RetryTimeoutOptions
  timeout?: number
}

export type FetchFromRegistryOptions = RequestInit & {
  authHeaderValue?: string
  compress?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
  abort?: () => void
}

export type FetchFromRegistry = (
  url: string,
  opts?: FetchFromRegistryOptions
) => Promise<Response>

export type GetAuthHeader = (uri: string) => string | undefined
