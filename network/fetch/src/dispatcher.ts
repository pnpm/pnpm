import { URL } from 'url'
import { Agent, ProxyAgent, type Dispatcher } from 'undici'
import { LRUCache } from 'lru-cache'
import { type SslConfig } from '@pnpm/types'

const DEFAULT_MAX_SOCKETS = 50

const DISPATCHER_CACHE = new LRUCache<string, Dispatcher>({ max: 50 })

export interface DispatcherOptions {
  ca?: string | string[] | Buffer
  cert?: string | string[] | Buffer
  key?: string | Buffer
  localAddress?: string
  maxSockets?: number
  strictSsl?: boolean
  timeout?: number
  httpProxy?: string
  httpsProxy?: string
  noProxy?: boolean | string
  clientCertificates?: Record<string, SslConfig>
}

/**
 * Clear the dispatcher cache. Useful for testing.
 */
export function clearDispatcherCache (): void {
  DISPATCHER_CACHE.clear()
}

/**
 * Get a dispatcher for the given URI and options.
 * Returns undefined if no special configuration is needed (to use global dispatcher).
 */
export function getDispatcher (uri: string, opts: DispatcherOptions): Dispatcher | undefined {
  // If no special options are set, use the global dispatcher
  if (!needsCustomDispatcher(opts)) {
    return undefined
  }

  if ((opts.httpProxy || opts.httpsProxy) && !checkNoProxy(uri, opts)) {
    const proxyDispatcher = getProxyDispatcher(uri, opts)
    if (proxyDispatcher) return proxyDispatcher
  }
  return getNonProxyDispatcher(uri, opts)
}

function needsCustomDispatcher (opts: DispatcherOptions): boolean {
  // Need custom dispatcher for:
  // - proxy configuration
  // - custom SSL/TLS certificates
  // - local address binding
  // - disabling strict SSL
  // - custom maxSockets
  // Note: timeout is NOT included here because it's handled via AbortController
  // in fetch.ts for request-level timeout. This allows tests using MockAgent
  // with setGlobalDispatcher() to work properly.
  return Boolean(
    opts.httpProxy ||
    opts.httpsProxy ||
    opts.ca ||
    opts.cert ||
    opts.key ||
    opts.localAddress ||
    opts.strictSsl === false ||
    opts.clientCertificates ||
    opts.maxSockets
  )
}

function getProxyDispatcher (uri: string, opts: DispatcherOptions): Dispatcher | null {
  const parsedUri = new URL(uri)
  const isHttps = parsedUri.protocol === 'https:'
  const proxy = isHttps ? opts.httpsProxy : opts.httpProxy

  if (!proxy) return null

  const sslConfig = pickSslConfigByUrl(opts.clientCertificates, uri)
  const { ca, cert, key: certKey } = { ...opts, ...sslConfig }

  const key = [
    `proxy:${proxy}`,
    `https:${isHttps.toString()}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && ca?.toString()) || '>no-ca<'}`,
    `cert:${(isHttps && cert?.toString()) || '>no-cert<'}`,
    `key:${(isHttps && certKey?.toString()) || '>no-key<'}`,
  ].join(':')

  if (DISPATCHER_CACHE.has(key)) {
    return DISPATCHER_CACHE.get(key)!
  }

  const proxyAgent = new ProxyAgent({
    uri: proxy,
    requestTls: isHttps
      ? {
        ca: ca as string | undefined,
        cert: cert as string | undefined,
        key: certKey as string | undefined,
        rejectUnauthorized: opts.strictSsl ?? true,
      }
      : undefined,
  })

  DISPATCHER_CACHE.set(key, proxyAgent)
  return proxyAgent
}

function getNonProxyDispatcher (uri: string, opts: DispatcherOptions): Dispatcher {
  const parsedUri = new URL(uri)
  const isHttps = parsedUri.protocol === 'https:'

  const sslConfig = pickSslConfigByUrl(opts.clientCertificates, uri)
  const { ca, cert, key: certKey } = { ...opts, ...sslConfig }

  const key = [
    `https:${isHttps.toString()}`,
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && ca?.toString()) || '>no-ca<'}`,
    `cert:${(isHttps && cert?.toString()) || '>no-cert<'}`,
    `key:${(isHttps && certKey?.toString()) || '>no-key<'}`,
  ].join(':')

  if (DISPATCHER_CACHE.has(key)) {
    return DISPATCHER_CACHE.get(key)!
  }

  const connectTimeout = typeof opts.timeout !== 'number' || opts.timeout === 0
    ? 0
    : opts.timeout + 1

  // Match agentkeepalive defaults:
  // - freeSocketTimeout: 4000 (idle socket timeout)
  // - keepAliveMsecs: 1000 (TCP keep-alive probe interval)
  // - maxFreeSockets: 256
  const agent = new Agent({
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    connectTimeout,
    keepAliveTimeout: 4000, // matches agentkeepalive's freeSocketTimeout
    keepAliveMaxTimeout: 15000, // max time to keep socket alive
    connect: isHttps
      ? {
        ca: ca as string | undefined,
        cert: cert as string | undefined,
        key: certKey as string | undefined,
        rejectUnauthorized: opts.strictSsl ?? true,
        localAddress: opts.localAddress,
      }
      : {
        localAddress: opts.localAddress,
      },
  })

  DISPATCHER_CACHE.set(key, agent)
  return agent
}

function checkNoProxy (uri: string, opts: { noProxy?: boolean | string }): boolean {
  const host = new URL(uri).hostname
    .split('.')
    .filter(x => x)
    .reverse()
  if (typeof opts.noProxy === 'string') {
    const noproxyArr = opts.noProxy.split(/\s*,\s*/g)
    return noproxyArr.some(no => {
      const noParts = no
        .split('.')
        .filter(x => x)
        .reverse()
      if (noParts.length === 0) {
        return false
      }
      for (let i = 0; i < noParts.length; i++) {
        if (host[i] !== noParts[i]) {
          return false
        }
      }
      return true
    })
  }
  return opts.noProxy === true
}

function pickSslConfigByUrl (
  sslConfigs: Record<string, SslConfig> | undefined,
  uri: string
): SslConfig | undefined {
  if (!sslConfigs) return undefined

  const parsedUri = new URL(uri)
  const host = parsedUri.host
  const hostWithoutPort = parsedUri.hostname

  // Try exact match with host (including port)
  const hostKey = `//${host}/`
  if (sslConfigs[hostKey]) return sslConfigs[hostKey]

  // Try match without port
  const hostWithoutPortKey = `//${hostWithoutPort}/`
  if (sslConfigs[hostWithoutPortKey]) return sslConfigs[hostWithoutPortKey]

  // Try matching by iterating through keys
  for (const key of Object.keys(sslConfigs)) {
    if (uri.includes(key.replace(/^\/\//, '').replace(/\/$/, ''))) {
      return sslConfigs[key]
    }
  }

  return undefined
}
