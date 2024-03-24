import { URL } from 'node:url'

import { getAgent, type AgentOptions } from '@pnpm/network.agent'
import {
  isRedirect,
  type Response,
  type HeadersInit,
  type FetchFromRegistry,
  type RetryTimeoutOptions,
  type FetchWithAgentOptions,
} from '@pnpm/types'

import { fetch } from './fetch.js'

const USER_AGENT: string = 'pnpm' as const // or maybe make it `${pkg.name}/${pkg.version} (+https://npm.im/${pkg.name})`

const ABBREVIATED_DOC =
  'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*'
const JSON_DOC = 'application/json'
const MAX_FOLLOWED_REDIRECTS = 20

export function fetchWithAgent(url: URL, opts: FetchWithAgentOptions): Promise<Response> {
  const agent = getAgent(url.toString(), {
    ...opts.agentOptions,
    strictSsl: opts.agentOptions.strictSsl ?? true,
  })

  const headers = opts.headers ?? {}

  // @ts-ignore
  headers.connection = agent ? 'keep-alive' : 'close'

  return fetch(url, {
    ...opts,
    agent,
  })
}

export function createFetchFromRegistry(
  defaultOpts: {
    fullMetadata?: boolean | undefined
    userAgent?: string | undefined
  } & AgentOptions
): FetchFromRegistry {
  return async (url: string, opts: {
    authHeaderValue?: string | undefined;
    compress?: boolean | undefined;
    retry?: RetryTimeoutOptions | undefined;
    timeout: number;
  } | undefined): Promise<Response> => {
    const headers: HeadersInit = {
      'user-agent': USER_AGENT,
      ...getHeaders({
        auth: opts?.authHeaderValue,
        fullMetadata: defaultOpts.fullMetadata,
        userAgent: defaultOpts.userAgent ?? USER_AGENT,
      }),
    }

    let redirects = 0

    let urlObject = new URL(url)

    const originalHost = urlObject.host

    while (true) {
      const agentOptions: AgentOptions = {
        ...defaultOpts,
        ...opts,
        strictSsl: defaultOpts.strictSsl ?? true,
      }

      // We should pass a URL object to node-fetch till this is not resolved:
      // https://github.com/bitinn/node-fetch/issues/245
      // eslint-disable-next-line no-await-in-loop
      const response = await fetchWithAgent(urlObject, {
        agentOptions,
        // if verifying integrity, node-fetch must not decompress
        compress: opts?.compress ?? false,
        headers,
        redirect: 'manual',
        retry: opts?.retry,
        timeout: opts?.timeout ?? 60_000,
      })

      if (!isRedirect(response.status) || redirects >= MAX_FOLLOWED_REDIRECTS) {
        return response
      }

      // This is a workaround to remove authorization headers on redirect.
      // Related pnpm issue: https://github.com/pnpm/pnpm/issues/1815
      redirects++

      const loc = response.headers.get('location')

      if (!loc) {
        continue
      }

      urlObject = new URL(loc)

      if (!headers.authorization || originalHost === urlObject.host) {
        continue
      }

      delete headers.authorization
    }
  }
}

function getHeaders(opts: {
  auth?: string | undefined
  fullMetadata?: boolean | undefined
  userAgent: string
}): HeadersInit {
  const headers: HeadersInit = {
    accept: opts.fullMetadata === true ? JSON_DOC : ABBREVIATED_DOC,
  }

  if (opts.auth) {
    headers.authorization = opts.auth
  }

  if (opts.userAgent) {
    headers['user-agent'] = opts.userAgent
  }

  return headers
}
