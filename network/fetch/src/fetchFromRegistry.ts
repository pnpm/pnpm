import { URL } from 'url'
import { readFileSync } from 'fs'
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

export function fetchWithAgent (url: RequestInfo, opts: FetchWithAgentOptions) {
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

function getUserCertificates (authOptions: Record<string, string>) {
  // Get all the auth options that have :certfile or :keyfile in their name
  const certAuths: {
    [registry: string]: {
      ca?: string
      cert: string
      key: string
    }
  } = {}

  for (const [key, value] of Object.entries(authOptions)) {
    if (key.includes(':certfile') || key.includes(':keyfile') || key.includes(':cafile')) {
      // Split by '/:' because the registry may contain a port
      const registry = key.split('/:')[0] + '/'
      if (!certAuths[registry]) {
        certAuths[registry] = { cert: '', key: '' }
      }

      if (key.includes(':certfile')) {
        certAuths[registry].cert = readFileSync(value, 'utf8')
      } else if (key.includes(':keyfile')) {
        certAuths[registry].key = readFileSync(value, 'utf8')
      } else if (key.includes(':cafile')) {
        certAuths[registry].ca = readFileSync(value, 'utf8')
      }
    }
  }

  return certAuths
}

export function createFetchFromRegistry (
  defaultOpts: {
    fullMetadata?: boolean
    userAgent?: string
    rawConfig?: Record<string, string>
  } & AgentOptions
): FetchFromRegistry {
  const clientCerts = getUserCertificates(defaultOpts.rawConfig ?? {})
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
          clientCertificates: clientCerts,
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

function getHeaders (
  opts: {
    auth?: string
    fullMetadata?: boolean
    userAgent?: string
  }
) {
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
