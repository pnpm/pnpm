/// <reference types="node-fetch" />
interface URL {
  href: string
}

export {
  FetchError,
  Headers,
  HeadersInit,
  Request,
  RequestInit,
  RequestContext,
  RequestMode,
  RequestRedirect,
  RequestCredentials,
  RequestCache,
  Response,
  ResponseType,
  ResponseInit,
} from 'node-fetch'

export type RequestInfo = string | URL | Request

export default function fetch(
  url: RequestInfo,
  init?: RequestInit
): Promise<Response>
