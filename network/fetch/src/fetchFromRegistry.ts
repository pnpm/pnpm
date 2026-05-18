import { URL } from 'node:url'

import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { RegistryConfig } from '@pnpm/types'

import { type ClientCertificates, type DispatcherOptions, getDispatcher } from './dispatcher.js'
import { fetch, isRedirect, type RequestInit } from './fetch.js'

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

export function createFetchFromRegistry (defaultOpts: CreateFetchFromRegistryOptions): FetchFromRegistry {
  const clientCertificates = extractTlsConfigs(defaultOpts.configByUri)
  return async (url, opts): Promise<Response> => {
    const headers: Record<string, string> = {
      'user-agent': USER_AGENT,
      ...getHeaders({
        auth: opts?.authHeaderValue,
        fullMetadata: opts?.fullMetadata,
        userAgent: defaultOpts.userAgent,
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
      const optsHeaders = opts.headers instanceof Headers
        ? Object.fromEntries(opts.headers.entries())
        : Array.isArray(opts.headers)
          ? Object.fromEntries(opts.headers)
          : opts.headers
      Object.assign(headers, optsHeaders)
    }

    let redirects = 0
    let urlObject = new URL(url)
    const originalHost = urlObject.host
    /* eslint-disable no-await-in-loop */
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
        timeout: opts?.timeout ?? 60000,
      })
      if (!isRedirect(response.status) || redirects >= MAX_FOLLOWED_REDIRECTS) {
        return response
      }

      redirects++
      // This is a workaround to remove authorization headers on redirect.
      // Related pnpm issue: https://github.com/pnpm/pnpm/issues/1815
      urlObject = resolveRedirectUrl(response, urlObject)
      if (!headers['authorization'] || originalHost === urlObject.host) continue
      delete headers.authorization
    }
    /* eslint-enable no-await-in-loop */
  }
}

interface Headers {
  accept: string
  authorization?: string
  'user-agent'?: string
}

function getHeaders (
  opts: {
    auth?: string
    fullMetadata?: boolean
    userAgent?: string
  }
): Headers {
  const headers: { accept: string, authorization?: string, 'user-agent'?: string } = {
    accept: opts.fullMetadata === true ? ACCEPT_FULL_DOC : ACCEPT_ABBREVIATED_DOC,
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
    throw new Error(`Redirect location header missing for ${currentUrl.toString()}`)
  }
  return new URL(location, currentUrl)
}
