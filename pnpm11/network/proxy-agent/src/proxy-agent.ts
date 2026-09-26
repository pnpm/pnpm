import type { Agent, ClientRequest } from 'node:http'
import type { Socket } from 'node:net'

import { PnpmError } from '@pnpm/error'
import { HttpProxyAgent } from 'http-proxy-agent'
import { HttpsProxyAgent, type HttpsProxyAgentOptions } from 'https-proxy-agent'
import { LRUCache } from 'lru-cache'
import { SocksProxyAgent } from 'socks-proxy-agent'

const DEFAULT_MAX_SOCKETS = 50

const AGENT_CACHE = new LRUCache<string, Agent>({ max: 50 })

export interface ProxyAgentOptions {
  ca?: string | string[]
  cert?: string | string[]
  httpProxy?: string
  httpsProxy?: string
  key?: string
  localAddress?: string
  maxSockets?: number
  noProxy?: boolean | string
  strictSsl?: boolean
  timeout?: number
  clientCertificates?: {
    [registryUrl: string]: {
      cert: string
      key: string
      ca?: string
    }
  }
}

export function getProxyAgent (uri: string, opts: ProxyAgentOptions): Agent | undefined {
  const parsedUri = new URL(uri)
  const proxyUri = getProxyUri(parsedUri, opts)
  if (!proxyUri) return
  const isHttps = parsedUri.protocol === 'https:'
  // Node.js verifies certificates unless told otherwise, so an omitted
  // strictSsl must not share a cached agent with an explicit false.
  const strictSsl = opts.strictSsl ?? true
  const maxSockets = opts.maxSockets ?? DEFAULT_MAX_SOCKETS
  const timeout = toAgentTimeout(opts.timeout)

  const key = [
    `https:${isHttps.toString()}`,
    `proxy:${proxyUri.protocol}//${proxyUri.username}:${proxyUri.password}@${proxyUri.host}:${proxyUri.port}`,
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `max-sockets:${maxSockets}`,
    `timeout:${timeout}`,
    `strict-ssl:${
      isHttps ? strictSsl.toString() : '>no-strict-ssl<'
    }`,
    `ca:${(isHttps && opts.ca?.toString()) || '>no-ca<'}`,
    `cert:${(isHttps && opts.cert?.toString()) || '>no-cert<'}`,
    `key:${(isHttps && opts.key) || '>no-key<'}`,
  ].join(':')

  if (AGENT_CACHE.peek(key)) {
    return AGENT_CACHE.get(key)
  }
  const proxy = getProxy(proxyUri, { ...opts, maxSockets, strictSsl, timeout }, isHttps)
  if (proxy) AGENT_CACHE.set(key, proxy)
  return proxy
}

function getProxyUri (
  uri: URL,
  opts: {
    httpProxy?: string
    httpsProxy?: string
  }
): URL | undefined {
  const { protocol } = uri

  let proxy: string | undefined
  switch (protocol) {
    case 'http:': {
      proxy = opts.httpProxy
      break
    }
    case 'https:': {
      proxy = opts.httpsProxy
      break
    }
  }

  if (!proxy) {
    return undefined
  }

  if (!proxy.includes('://')) {
    proxy = `${protocol}//${proxy}`
  }

  if (typeof proxy !== 'string') {
    return proxy
  }

  try {
    return new URL(proxy)
  } catch {
    throw new PnpmError('INVALID_PROXY', "Couldn't parse proxy URL", {
      hint: 'If your proxy URL contains a username and password, make sure to URL-encode them (you may use the encodeURIComponent function). For instance, https-proxy=https://use%21r:pas%2As@my.proxy:1234/foo. Do not encode the colon (:) between the username and password.',
    })
  }
}

function getProxy (
  proxyUrl: URL,
  opts: {
    ca?: string | string[]
    cert?: string | string[]
    key?: string
    timeout: number
    localAddress?: string
    maxSockets: number
    strictSsl: boolean
  },
  isHttps: boolean
) {
  const proxyOpts = {
    auth: getAuth(proxyUrl),
    ca: opts.ca,
    cert: opts.cert,
    key: opts.key,
    localAddress: opts.localAddress,
    maxSockets: opts.maxSockets,
    rejectUnauthorized: opts.strictSsl,
    timeout: opts.timeout,
  }

  if (proxyUrl.protocol === 'http:' || proxyUrl.protocol === 'https:') {
    if (!isHttps) {
      return new HttpProxyAgent(proxyUrl, proxyOpts)
    } else {
      return new PatchedHttpsProxyAgent(proxyUrl, proxyOpts)
    }
  }
  if (proxyUrl.protocol?.startsWith('socks')) {
    return new SocksProxyAgent(proxyUrl, proxyOpts)
  }
  return undefined
}

function toAgentTimeout (timeout: number | undefined): number {
  return typeof timeout !== 'number' || timeout === 0 ? 0 : timeout + 1
}

function getAuth (user: { username?: string, password?: string }) {
  if (!user.username) {
    return undefined
  }
  let auth = user.username
  if (user.password) {
    auth += `:${user.password}`
  }
  return decodeURIComponent(auth)
}

type AgentConnectOpts = Parameters<HttpsProxyAgent<string>['connect']>[1]

// This is a workaround for this issue: https://github.com/TooTallNate/node-https-proxy-agent/issues/89
class PatchedHttpsProxyAgent<Uri extends string> extends HttpsProxyAgent<Uri> {
  readonly #extraOpts: HttpsProxyAgentOptions<Uri>

  constructor (proxyUrl: Uri | URL, opts: HttpsProxyAgentOptions<Uri>) {
    super(proxyUrl, opts)
    this.#extraOpts = opts
  }

  override async connect (req: ClientRequest, opts: AgentConnectOpts): Promise<Socket> {
    return super.connect(req, { ...this.#extraOpts, ...opts } as AgentConnectOpts)
  }
}
