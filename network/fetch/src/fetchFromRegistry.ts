import { URL } from 'node:url'

import { isRedirect } from 'node-fetch'

import type { FetchWithAgentOptions } from '@pnpm/types'
import { getAgent, type AgentOptions } from '@pnpm/network.agent'
import type { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching-types'

import { fetch } from './fetch'

const USER_AGENT = 'pnpm' // or maybe make it `${pkg.name}/${pkg.version} (+https://npm.im/${pkg.name})`

const ABBREVIATED_DOC =
  'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*'
const JSON_DOC = 'application/json'
const MAX_FOLLOWED_REDIRECTS = 20

export function fetchWithAgent(url: RequestInfo, opts: FetchWithAgentOptions) {
  const agent = getAgent(url.toString(), {
    ...opts.agentOptions,
    strictSsl: opts.agentOptions.strictSsl ?? true,
  } as any) // eslint-disable-line

  const headers = opts.headers ?? {}

  headers.connection = agent ? 'keep-alive' : 'close'

  return fetch(url, {
    ...opts,
    agent,
  })
}

export type { AgentOptions }

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
    timeout?: number | undefined;
  } | undefined): Promise<Response> => {
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

    while (true) {
      const agentOptions = {
        ...defaultOpts,
        ...opts,
        strictSsl: defaultOpts.strictSsl ?? true,
      } as any // eslint-disable-line

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
        timeout: opts?.timeout ?? 60000,
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
  auth?: string
  fullMetadata?: boolean
  userAgent?: string
}) {
  const headers: {
    accept: string
    authorization?: string
    'user-agent'?: string
  } = {
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
