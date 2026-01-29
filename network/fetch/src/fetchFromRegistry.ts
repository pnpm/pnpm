import { URL } from 'url'
import { type SslConfig } from '@pnpm/types'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { fetch, isRedirect, type RequestInit } from './fetch.js'
import { getDispatcher, type DispatcherOptions } from './dispatcher.js'

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

export type { DispatcherOptions }

export interface CreateFetchFromRegistryOptions extends DispatcherOptions {
  userAgent?: string
  sslConfigs?: Record<string, SslConfig>
}

export function createFetchFromRegistry (defaultOpts: CreateFetchFromRegistryOptions): FetchFromRegistry {
  return async (url, opts): Promise<Response> => {
    const headers: Record<string, string> = {
      'user-agent': USER_AGENT,
      ...getHeaders({
        auth: opts?.authHeaderValue,
        fullMetadata: opts?.fullMetadata,
        userAgent: defaultOpts.userAgent,
      }),
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
        clientCertificates: defaultOpts.sslConfigs,
      }

      const response = await fetchWithDispatcher(urlObject, {
        dispatcherOptions,
        // if verifying integrity, native fetch must not decompress
        headers,
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

function resolveRedirectUrl (response: Response, currentUrl: URL): URL {
  const location = response.headers.get('location')
  if (!location) {
    throw new Error(`Redirect location header missing for ${currentUrl.toString()}`)
  }
  return new URL(location, currentUrl)
}
