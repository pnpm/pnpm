import { Request, RequestInit as NodeRequestInit, Response } from 'node-fetch'
export {
  FetchError,
  Headers,
  HeadersInit,
  RequestContext,
  RequestMode,
  RequestRedirect,
  RequestCredentials,
  RequestCache,
  ResponseType,
  ResponseInit,
} from 'node-fetch'

export {
  Request,
  Response,
}

interface URLLike {
  href: string
}

export interface RetryOpts {
  factor?: number
  maxTimeout?: number
  minTimeout?: number
  onRetry? (error: unknown): void
  retries?: number
}

export interface RequestInit extends NodeRequestInit {
  retry?: RetryOpts
  onRetry? (error: unknown, opts: RequestInit): void
}

export type RequestInfo = string | URLLike | Request

declare function fetch (
  url: RequestInfo,
  init?: RequestInit,
): Promise<Response>

declare namespace fetch {
  function isRedirect (code: number): boolean
}

export default fetch
