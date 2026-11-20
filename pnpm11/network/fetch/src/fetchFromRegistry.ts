import { URL } from 'node:url'

import { PnpmError, redactUrlCredentials } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { RegistryConfig } from '@pnpm/types'

import { type ClientCertificates, DEFAULT_FETCH_TIMEOUT, type DispatcherOptions, getConnectionOrigin, getDispatcher } from './dispatcher.js'
import { fetch, isRedirect, type RequestInit } from './fetch.js'
import { createOriginConcurrencyGates, type GetOriginConcurrencyGate } from './networkConcurrencyGate.js'

const USER_AGENT = 'pnpm' // or maybe make it `${pkg.name}/${pkg.version} (+https://npm.im/${pkg.name})`

const FULL_DOC = 'application/json'
const ACCEPT_FULL_DOC = `${FULL_DOC}; q=1.0, */*`

const ABBREVIATED_DOC = 'application/vnd.npm.install-v1+json'
const ACCEPT_ABBREVIATED_DOC = `${ABBREVIATED_DOC}; q=1.0, ${FULL_DOC}; q=0.8, */*`

const MAX_FOLLOWED_REDIRECTS = 20

export interface FetchWithDispatcherOptions extends RequestInit {
  dispatcherOptions: DispatcherOptions
}

export function fetchWithDispatcher (url: string | URL, opts: FetchWithDispatcherOptions): Promise<Response> {
  const dispatcher = getDispatcher(url.toString(), {
    ...opts.dispatcherOptions,
    strictSsl: opts.dispatcherOptions.strictSsl ?? true,
    timeout: opts.timeout ?? opts.dispatcherOptions.timeout,
  })
  return fetch(url, {
    ...opts,
    dispatcher,
  })
}

export interface CreateDispatchedFetchOptions extends DispatcherOptions {
  /**
   * Per-registry config (TLS, auth, etc.). When set, the matching TLS entries
   * are automatically extracted into `clientCertificates` so callers don't
   * have to do it themselves.
   */
  configByUri?: Record<string, RegistryConfig>
}

/**
 * Returns a {@link fetch} pre-bound to the given dispatcher options, so callers
 * that need a fetch function (rather than a one-shot call) can route their
 * requests through the configured proxy / TLS / local-address settings.
 */
export function createDispatchedFetch (opts: CreateDispatchedFetchOptions): (url: string | URL, opts?: RequestInit) => Promise<Response> {
  const dispatcherOptions: DispatcherOptions = {
    ...opts,
    clientCertificates: opts.clientCertificates ?? extractTlsConfigs(opts.configByUri),
  }
  return (url, fetchOpts) => fetchWithDispatcher(url, { ...fetchOpts, dispatcherOptions })
}

export type { DispatcherOptions }

export interface CreateFetchFromRegistryOptions extends DispatcherOptions {
  userAgent?: string
  configByUri?: Record<string, RegistryConfig>
}

type FetchFromRegistryRequestOptions = Parameters<FetchFromRegistry>[1]

export function createFetchFromRegistry (defaultOpts: CreateFetchFromRegistryOptions): FetchFromRegistry {
  const clientCertificates = extractTlsConfigs(defaultOpts.configByUri)
  const concurrencyGateFor = createOriginConcurrencyGates()
  return async (url, opts): Promise<Response> => fetchFollowingRedirects({
    url,
    opts,
    headers: createRequestHeaders(defaultOpts.userAgent, opts),
    defaultOpts,
    clientCertificates,
    concurrencyGateFor,
  })
}

function createRequestHeaders (userAgent: string | undefined, opts: FetchFromRegistryRequestOptions): Record<string, string> {
  const headers: Record<string, string> = {
    'user-agent': USER_AGENT,
    ...getHeaders({
      auth: opts?.authHeaderValue,
      fullMetadata: opts?.fullMetadata,
      method: opts?.method,
      userAgent,
    }),
  }
  if (opts?.ifNoneMatch) {
    headers['if-none-match'] = opts.ifNoneMatch
  }
  if (opts?.ifModifiedSince) {
    headers['if-modified-since'] = opts.ifModifiedSince
  }
  // Merge caller-provided headers (e.g. content-type, npm-otp) on top
  if (opts?.headers) {
    Object.assign(headers, toHeaderRecord(opts.headers))
  }
  return headers
}

function toHeaderRecord (headers: NonNullable<RequestInit['headers']>): object {
  if (headers instanceof Headers) return Object.fromEntries(headers.entries())
  if (Array.isArray(headers)) return Object.fromEntries(headers)
  return headers
}

interface RedirectFollowingRequest {
  url: string
  opts: FetchFromRegistryRequestOptions
  headers: Record<string, string>
  defaultOpts: CreateFetchFromRegistryOptions
  clientCertificates: ClientCertificates | undefined
  concurrencyGateFor: GetOriginConcurrencyGate
}

async function fetchFollowingRedirects ({ url, opts, headers, defaultOpts, clientCertificates, concurrencyGateFor }: RedirectFollowingRequest): Promise<Response> {
  let redirects = 0
  let urlObject = new URL(url)
  const originalOrigin = urlObject.origin
  /* eslint-disable no-await-in-loop -- each redirect hop needs the previous response */
  while (true) {
    const dispatcherOptions: DispatcherOptions = {
      ...defaultOpts,
      ...opts,
      strictSsl: defaultOpts.strictSsl ?? true,
      clientCertificates,
    }

    const response = await fetchWithDispatcher(urlObject, {
      dispatcherOptions,
      body: opts?.body,
      // if verifying integrity, native fetch must not decompress
      headers,
      method: opts?.method,
      redirect: 'manual',
      retry: opts?.retry,
      timeout: opts?.timeout ?? defaultOpts.timeout ?? DEFAULT_FETCH_TIMEOUT,
      concurrencyGate: concurrencyGateFor(getConnectionOrigin(urlObject, dispatcherOptions)),
    })
    if (
      opts?.redirect === 'manual' ||
      !isRedirect(response.status) ||
      redirects >= MAX_FOLLOWED_REDIRECTS
    ) {
      return response
    }

    redirects++
    // This is a workaround to remove authorization headers on redirect.
    // Related pnpm issue: https://github.com/pnpm/pnpm/issues/1815
    urlObject = resolveRedirectUrl(response, urlObject)
    // The permit stays with the body. Drop it before the next hop so a
    // redirect chain cannot pin a connection slot.
    await response.body?.cancel()
    if (originalOrigin !== urlObject.origin) {
      if (opts?.body != null) {
        throw new PnpmError('REDIRECT_BODY_CROSS_ORIGIN', `Cannot replay request body across origins: ${redactUrlCredentials(urlObject.toString())}`)
      }
      removeCredentialHeaders(headers)
    }
  }
  /* eslint-enable no-await-in-loop */
}

function removeCredentialHeaders (headers: Record<string, string>): void {
  if (headers['authorization']) {
    delete headers.authorization
  }
  delete headers['npm-otp']
}

interface Headers {
  accept?: string
  authorization?: string
  'user-agent'?: string
}

function getHeaders (
  opts: {
    auth?: string
    fullMetadata?: boolean
    method?: string
    userAgent?: string
  }
): Headers {
  const headers: Headers = {}
  // The abbreviated/full-metadata Accept header is meaningful only on package
  // metadata reads. Setting it on writes (PUT/POST/DELETE) breaks npmjs.org's
  // dist-tag endpoint, which rejects the request with a generic 400.
  if (!opts.method || opts.method === 'GET' || opts.method === 'HEAD') {
    headers.accept = opts.fullMetadata === true ? ACCEPT_FULL_DOC : ACCEPT_ABBREVIATED_DOC
  }
  if (opts.auth) {
    headers['authorization'] = opts.auth
  }
  if (opts.userAgent) {
    headers['user-agent'] = opts.userAgent
  }
  return headers
}

function extractTlsConfigs (configByUri?: Record<string, RegistryConfig>): ClientCertificates | undefined {
  if (!configByUri) return undefined
  let result: ClientCertificates | undefined
  for (const [uri, config] of Object.entries(configByUri)) {
    if (config.tls) {
      result ??= {}
      result[uri] = config.tls
    }
  }
  return result
}

function resolveRedirectUrl (response: Response, currentUrl: URL): URL {
  const location = response.headers.get('location')
  if (!location) {
    throw new Error(`Redirect location header missing for ${redactUrlCredentials(currentUrl.toString())}`)
  }
  return new URL(location, currentUrl)
}
