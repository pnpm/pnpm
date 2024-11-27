import { URL } from 'url'
import { type SslConfig } from '@pnpm/types'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { getAgent, type AgentOptions } from '@pnpm/network.agent'
import { fetch, isRedirect, type Response, type RequestInfo, type RequestInit } from './fetch'

const USER_AGENT = 'pnpm' // or maybe make it `${pkg.name}/${pkg.version} (+https://npm.im/${pkg.name})`

const ABBREVIATED_DOC = 'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*'
const JSON_DOC = 'application/json'
const MAX_FOLLOWED_REDIRECTS = 20

export type FetchWithAgentOptions = RequestInit & {
  agentOptions: AgentOptions
}

export function fetchWithAgent (url: RequestInfo, opts: FetchWithAgentOptions): Promise<Response> {
  const agent = getAgent(url.toString(), {
    ...opts.agentOptions,
    strictSsl: opts.agentOptions.strictSsl ?? true,
  } as any) as any // eslint-disable-line
  const headers = opts.headers ?? {}
  // @ts-expect-error
  headers['connection'] = agent ? 'keep-alive' : 'close'
  return fetch(url, {
    ...opts,
    agent,
  })
}

export type { AgentOptions }

export function createFetchFromRegistry (
  defaultOpts: {
    fullMetadata?: boolean
    userAgent?: string
    sslConfigs?: Record<string, SslConfig>
  } & AgentOptions
): FetchFromRegistry {
  return async (url, opts): Promise<Response> => {
    const headers = {
      'user-agent': USER_AGENT,
      ...getHeaders({
        auth: opts?.authHeaderValue,
        fullMetadata: defaultOpts.fullMetadata,
        userAgent: defaultOpts.userAgent,
      }),
    }

    let redirects = 0
    let urlObject = new URL(url)
    const originalHost = urlObject.host
    /* eslint-disable no-await-in-loop */
    while (true) {
      const agentOptions = {
        ...defaultOpts,
        ...opts,
        strictSsl: defaultOpts.strictSsl ?? true,
      } as any // eslint-disable-line

      // We should pass a URL object to node-fetch till this is not resolved:
      // https://github.com/bitinn/node-fetch/issues/245
      const response = await fetchWithAgent(urlObject, {
        agentOptions: {
          ...agentOptions,
          clientCertificates: defaultOpts.sslConfigs,
        },
        // if verifying integrity, node-fetch must not decompress
        compress: opts?.compress ?? false,
        headers,
        redirect: 'manual',
        retry: opts?.retry,
        timeout: opts?.timeout ?? 60000,
      })
      if (!isRedirect(response.status) || redirects >= MAX_FOLLOWED_REDIRECTS) {
        return response
      }

      // This is a workaround to remove authorization headers on redirect.
      // Related pnpm issue: https://github.com/pnpm/pnpm/issues/1815
      redirects++
      urlObject = new URL(response.headers.get('location')!)
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
    accept: opts.fullMetadata === true ? JSON_DOC : ABBREVIATED_DOC,
  }
  if (opts.auth) {
    headers['authorization'] = opts.auth
  }
  if (opts.userAgent) {
    headers['user-agent'] = opts.userAgent
  }
  return headers
}
