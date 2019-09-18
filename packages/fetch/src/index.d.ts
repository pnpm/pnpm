/// <reference types="node-fetch" />
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
  minTimeout?: number
  retries?: number
  factor?: number
  onRetry?(error: any): void
}

export interface RequestInit extends NodeRequestInit {
  retry?: RetryOpts
  onRetry?(error: any, opts: RequestInit): void
}

export type RequestInfo = string | URLLike | Request

declare function fetch(
  url: RequestInfo,
  init?: RequestInit
): Promise<Response>

declare namespace fetch {
  function isRedirect(code: number): boolean;
}

export default fetch;
