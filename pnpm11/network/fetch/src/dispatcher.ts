import net from 'node:net'
import tls from 'node:tls'
import { URL } from 'node:url'

import { nerfDart } from '@pnpm/config.registry-auth-key'
import { PnpmError } from '@pnpm/error'
import type { TlsConfig } from '@pnpm/types'
import { LRUCache } from 'lru-cache'
import { SocksClient } from 'socks'
import { Agent, type Dispatcher, getGlobalDispatcher, ProxyAgent, setGlobalDispatcher } from 'undici'

const DEFAULT_MAX_SOCKETS = 50
const KEEP_ALIVE_TIMEOUT = 30_000 // 30 seconds
const KEEP_ALIVE_MAX_TIMEOUT = 600_000 // 10 minutes

/**
 * Default of the `fetch-timeout` setting: how long a request may make no
 * progress before it fails.
 *
 * It is an inactivity timeout, not a deadline for the whole request: undici
 * restarts the body timer on every chunk that arrives, so a big tarball may
 * take as long as the connection needs. A total deadline would abort healthy
 * downloads on slow connections (https://github.com/pnpm/pnpm/issues/14604).
 */
export const DEFAULT_FETCH_TIMEOUT = 60_000

/**
 * Longest delay Node's timers accept. The socks library has no way to turn its
 * handshake timeout off, so a disabled timeout (`0`) is expressed as a delay
 * that no request outlives.
 */
const MAX_TIMER_DELAY = 2_147_483_647

// Set an optimized global dispatcher so that requests without custom options
// (no proxy, no custom certs) still benefit from better keep-alive and Happy Eyeballs.
//
// Note: we intentionally do NOT enable HTTP/2 (allowH2) or HTTP/1.1 pipelining here.
// With HTTP/2, undici multiplexes many streams over 1-2 TCP connections sharing a single
// congestion window. In benchmarks this was slower than opening ~50 independent HTTP/1.1
// connections that each get their own congestion window and can saturate bandwidth in parallel.
const GLOBAL_DISPATCHER = new Agent({
  connections: DEFAULT_MAX_SOCKETS,
  keepAliveTimeout: KEEP_ALIVE_TIMEOUT,
  keepAliveMaxTimeout: KEEP_ALIVE_MAX_TIMEOUT,
  headersTimeout: DEFAULT_FETCH_TIMEOUT,
  bodyTimeout: DEFAULT_FETCH_TIMEOUT,
  connect: {
    autoSelectFamily: true,
  },
}).compose(stripSecFetchHeaders)

setGlobalDispatcher(GLOBAL_DISPATCHER)

// undici's fetch() automatically adds sec-fetch-* headers (e.g. sec-fetch-mode: cors)
// per the Fetch spec. Some registries like Azure DevOps Artifacts interpret these as
// browser requests and reject them with HTTP 400. Since pnpm is a CLI tool, these
// headers serve no purpose and must be stripped.
// See https://github.com/pnpm/pnpm/issues/11572
function stripSecFetchHeaders (dispatch: Dispatcher['dispatch']): Dispatcher['dispatch'] {
  return (opts, handler) => {
    if (opts.headers) {
      if (Array.isArray(opts.headers)) {
        // Flat array format: [key1, val1, key2, val2, ...]
        const filtered: string[] = []
        for (let i = 0; i < opts.headers.length; i += 2) {
          if (!opts.headers[i].toLowerCase().startsWith('sec-fetch-')) {
            filtered.push(opts.headers[i], opts.headers[i + 1])
          }
        }
        opts = { ...opts, headers: filtered }
      } else if (typeof opts.headers === 'object') {
        // undici also accepts an iterable of [key, value] pairs (e.g. a Map or
        // web Headers). Use that iterator when present; otherwise fall back to
        // Object.entries for plain IncomingHttpHeaders objects.
        const entries = Symbol.iterator in opts.headers
          ? (opts.headers as Iterable<[string, string | string[] | undefined]>)
          : Object.entries(opts.headers as Record<string, string | string[] | undefined>)
        const headers: Record<string, string | string[] | undefined> = {}
        for (const [key, value] of entries) {
          if (!key.toLowerCase().startsWith('sec-fetch-')) {
            headers[key] = value
          }
        }
        opts = { ...opts, headers }
      }
    }
    return dispatch(opts, handler)
  }
}

const DISPATCHER_CACHE = new LRUCache<string, Dispatcher>({
  max: 50,
  dispose: (dispatcher) => {
    if (typeof (dispatcher as Agent).close === 'function') {
      void (dispatcher as Agent).close()
    }
  },
})

export type ClientCertificates = Record<string, TlsConfig>

export interface DispatcherOptions {
  ca?: string | string[] | Buffer
  cert?: string | string[] | Buffer
  key?: string | Buffer
  localAddress?: string
  maxSockets?: number
  strictSsl?: boolean
  /**
   * How long the request may make no progress before it fails, in
   * milliseconds. `0` disables it. Defaults to {@link DEFAULT_FETCH_TIMEOUT}.
   */
  timeout?: number
  httpProxy?: string
  httpsProxy?: string
  noProxy?: boolean | string
  clientCertificates?: ClientCertificates
}

/**
 * Clear the dispatcher cache. Useful for testing.
 */
export function clearDispatcherCache (): void {
  DISPATCHER_CACHE.clear()
}

/**
 * Destroy the global dispatcher and every cached dispatcher, closing their open
 * sockets. Intended for process shutdown only: once called, the module can no
 * longer perform network requests. This is used to work around a Windows crash
 * that happens when the process exits while sockets are still open
 * (https://github.com/nodejs/node/issues/56645).
 */
export async function destroyDispatchers (): Promise<void> {
  // getGlobalDispatcher() is included in case something replaced GLOBAL_DISPATCHER
  // via setGlobalDispatcher(); the Set removes the duplicate when it is still our own instance.
  const dispatchers = new Set<Dispatcher>([
    GLOBAL_DISPATCHER,
    getGlobalDispatcher(),
    ...DISPATCHER_CACHE.values(),
  ])
  await Promise.allSettled(Array.from(dispatchers, dispatcher => dispatcher.destroy()))
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

  const parsedUri = new URL(uri)

  if ((opts.httpProxy || opts.httpsProxy) && !checkNoProxy(parsedUri, opts)) {
    const proxyDispatcher = getProxyDispatcher(parsedUri, opts)
    if (proxyDispatcher) return proxyDispatcher
  }
  return getNonProxyDispatcher(parsedUri, opts)
}

function inactivityTimeout (opts: DispatcherOptions): number {
  return opts.timeout ?? DEFAULT_FETCH_TIMEOUT
}

function hasClientCertificates (certs?: ClientCertificates): boolean {
  if (!certs) return false
  for (const uri in certs) {
    const entry = certs[uri]
    if (entry.cert || entry.key || entry.ca) return true
  }
  return false
}

function needsCustomDispatcher (opts: DispatcherOptions): boolean {
  return Boolean(
    opts.httpProxy ||
    opts.httpsProxy ||
    opts.ca ||
    opts.cert ||
    opts.key ||
    opts.localAddress ||
    opts.strictSsl === false ||
    hasClientCertificates(opts.clientCertificates) ||
    opts.maxSockets
  )
}

function parseProxyUrl (proxy: string, protocol: string): URL {
  let proxyUrl = proxy
  if (!proxyUrl.includes('://')) {
    proxyUrl = `${protocol}//${proxyUrl}`
  }
  try {
    return new URL(proxyUrl)
  } catch {
    throw new PnpmError('INVALID_PROXY', "Couldn't parse proxy URL", {
      hint: 'If your proxy URL contains a username and password, make sure to URL-encode them ' +
        '(you may use the encodeURIComponent function). For instance, ' +
        'https-proxy=https://use%21r:pas%2As@my.proxy:1234/foo. ' +
        'Do not encode the colon (:) between the username and password.',
    })
  }
}


function getSocksProxyType (protocol: string): 4 | 5 {
  switch (protocol.replace(':', '')) {
    case 'socks4':
    case 'socks4a':
      return 4
    default:
      return 5
  }
}

function getProxyDispatcher (parsedUri: URL, opts: DispatcherOptions): Dispatcher | null {
  const isHttps = parsedUri.protocol === 'https:'
  const proxy = isHttps ? opts.httpsProxy : opts.httpProxy

  if (!proxy) return null

  const proxyUrl = parseProxyUrl(proxy, parsedUri.protocol)

  const sslConfig = pickSettingByUrl(opts.clientCertificates, parsedUri.href)
  const { ca, cert, key: certKey } = { ...opts, ...sslConfig }

  const key = [
    `proxy:${proxyUrl.protocol}//${proxyUrl.username}:${proxyUrl.password}@${proxyUrl.host}:${proxyUrl.port}`,
    `https:${isHttps.toString()}`,
    `timeout:${inactivityTimeout(opts).toString()}`,
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `max-sockets:${(opts.maxSockets ?? DEFAULT_MAX_SOCKETS).toString()}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && ca?.toString()) || '-'}`,
    `cert:${(isHttps && cert?.toString()) || '-'}`,
    `key:${(isHttps && certKey?.toString()) || '-'}`,
  ].join(':')

  if (DISPATCHER_CACHE.has(key)) {
    return DISPATCHER_CACHE.get(key)!
  }

  let dispatcher: Dispatcher

  if (proxyUrl.protocol.startsWith('socks')) {
    dispatcher = createSocksDispatcher(proxyUrl, parsedUri, opts, { ca, cert, key: certKey })
  } else {
    dispatcher = createHttpProxyDispatcher(proxyUrl, isHttps, opts, { ca, cert, key: certKey })
  }

  dispatcher = dispatcher.compose(stripSecFetchHeaders)
  DISPATCHER_CACHE.set(key, dispatcher)
  return dispatcher
}

function createHttpProxyDispatcher (
  proxyUrl: URL,
  isHttps: boolean,
  opts: DispatcherOptions,
  tlsConfig: { ca?: string | string[] | Buffer, cert?: string | string[] | Buffer, key?: string | Buffer }
): Dispatcher {
  return new ProxyAgent({
    uri: proxyUrl.href,
    token: proxyUrl.username
      ? `Basic ${Buffer.from(`${decodeURIComponent(proxyUrl.username)}:${decodeURIComponent(proxyUrl.password)}`).toString('base64')}`
      : undefined,
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    connectTimeout: inactivityTimeout(opts),
    headersTimeout: inactivityTimeout(opts),
    bodyTimeout: inactivityTimeout(opts),
    keepAliveTimeout: KEEP_ALIVE_TIMEOUT,
    keepAliveMaxTimeout: KEEP_ALIVE_MAX_TIMEOUT,
    requestTls: isHttps
      ? {
        ca: tlsConfig.ca,
        cert: tlsConfig.cert,
        key: tlsConfig.key,
        rejectUnauthorized: opts.strictSsl ?? true,
        localAddress: opts.localAddress,
      }
      : undefined,
    proxyTls: {
      ca: opts.ca,
      rejectUnauthorized: opts.strictSsl ?? true,
    },
  })
}

function createSocksDispatcher (
  proxyUrl: URL,
  targetUri: URL,
  opts: DispatcherOptions,
  tlsConfig: { ca?: string | string[] | Buffer, cert?: string | string[] | Buffer, key?: string | Buffer }
): Dispatcher {
  const isHttps = targetUri.protocol === 'https:'
  const socksType = getSocksProxyType(proxyUrl.protocol)
  const proxyHost = proxyUrl.hostname
  const proxyPort = parseInt(proxyUrl.port, 10) || (socksType === 4 ? 1080 : 1080)
  const timeout = inactivityTimeout(opts)

  return new Agent({
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    headersTimeout: timeout,
    bodyTimeout: timeout,
    keepAliveTimeout: KEEP_ALIVE_TIMEOUT,
    keepAliveMaxTimeout: KEEP_ALIVE_MAX_TIMEOUT,
    // undici applies its own `connectTimeout` only to the connector it builds
    // itself, so this one bounds the SOCKS handshake and the TLS handshake
    // that follows it.
    connect: async (connectOpts, callback) => {
      // A timed-out handshake surfaces as an 'error' on a socket that may
      // already have been handed to undici, so every path settles at most once.
      let settled = false
      const claimSettle = (): boolean => {
        if (settled) return false
        settled = true
        return true
      }
      try {
        const { socket } = await SocksClient.createConnection({
          proxy: {
            host: proxyHost,
            port: proxyPort,
            type: socksType,
            userId: proxyUrl.username ? decodeURIComponent(proxyUrl.username) : undefined,
            password: proxyUrl.password ? decodeURIComponent(proxyUrl.password) : undefined,
          },
          command: 'connect',
          destination: {
            host: connectOpts.hostname!,
            port: parseInt(String(connectOpts.port!), 10),
          },
          timeout: timeout === 0 ? MAX_TIMER_DELAY : timeout,
        })

        if (isHttps) {
          const tlsOpts: tls.ConnectionOptions = {
            socket: socket as net.Socket,
            servername: connectOpts.hostname!,
            ca: tlsConfig.ca,
            cert: tlsConfig.cert,
            key: tlsConfig.key,
            rejectUnauthorized: opts.strictSsl ?? true,
          }
          const tlsSocket = tls.connect(tlsOpts)
          tlsSocket.setTimeout(timeout, () => {
            tlsSocket.destroy(new PnpmError('TLS_HANDSHAKE_TIMEOUT', `The TLS handshake with ${connectOpts.hostname!} through the SOCKS proxy timed out after ${timeout}ms`))
          })
          tlsSocket.on('secureConnect', () => {
            tlsSocket.setTimeout(0)
            if (claimSettle()) callback(null, tlsSocket)
          })
          tlsSocket.on('error', (err) => {
            if (claimSettle()) callback(err, null)
          })
        } else if (claimSettle()) {
          callback(null, socket as net.Socket)
        }
      } catch (err) {
        if (claimSettle()) callback(err as Error, null)
      }
    },
  })
}

function getNonProxyDispatcher (parsedUri: URL, opts: DispatcherOptions): Dispatcher {
  const isHttps = parsedUri.protocol === 'https:'

  const sslConfig = pickSettingByUrl(opts.clientCertificates, parsedUri.href)
  const { ca, cert, key: certKey } = { ...opts, ...sslConfig }

  const key = [
    `https:${isHttps.toString()}`,
    `timeout:${inactivityTimeout(opts).toString()}`,
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `max-sockets:${(opts.maxSockets ?? DEFAULT_MAX_SOCKETS).toString()}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && ca?.toString()) || '-'}`,
    `cert:${(isHttps && cert?.toString()) || '-'}`,
    `key:${(isHttps && certKey?.toString()) || '-'}`,
  ].join(':')

  if (DISPATCHER_CACHE.has(key)) {
    return DISPATCHER_CACHE.get(key)!
  }

  const timeout = inactivityTimeout(opts)

  const agent = new Agent({
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    connectTimeout: timeout,
    headersTimeout: timeout,
    bodyTimeout: timeout,
    keepAliveTimeout: KEEP_ALIVE_TIMEOUT,
    keepAliveMaxTimeout: KEEP_ALIVE_MAX_TIMEOUT,
    connect: isHttps
      ? {
        autoSelectFamily: true,
        ca,
        cert,
        key: certKey,
        rejectUnauthorized: opts.strictSsl ?? true,
        localAddress: opts.localAddress,
      }
      : {
        autoSelectFamily: true,
        localAddress: opts.localAddress,
      },
  })

  const dispatcher = agent.compose(stripSecFetchHeaders)
  DISPATCHER_CACHE.set(key, dispatcher)
  return dispatcher
}

function checkNoProxy (parsedUri: URL, opts: { noProxy?: boolean | string }): boolean {
  const host = parsedUri.hostname
    .split('.')
    .filter(x => x)
    .reverse()
  if (typeof opts.noProxy === 'string') {
    const noproxyArr = opts.noProxy.split(',').map(s => s.trim())
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

/**
 * Pick SSL/TLS configuration by URL using nerf-dart matching.
 * This matches the behavior of @pnpm/network.config's pickSettingByUrl.
 */
function pickSettingByUrl<T> (
  settings: Record<string, T> | undefined,
  uri: string
): T | undefined {
  if (!settings) return undefined

  // Try exact match first
  if (settings[uri]) return settings[uri]

  // Use nerf-dart format for matching (e.g., //registry.npmjs.org/)
  const nerf = nerfDart(uri)
  if (settings[nerf]) return settings[nerf]

  // Try without port
  const parsedUrl = new URL(uri)
  const withoutPort = removePort(parsedUrl)
  if (settings[withoutPort]) return settings[withoutPort]

  // Try progressively shorter nerf-dart paths
  const maxParts = Object.keys(settings).reduce((max, key) => {
    const parts = key.split('/').length
    return parts > max ? parts : max
  }, 0)
  const parts = nerf.split('/')
  for (let i = Math.min(parts.length, maxParts) - 1; i >= 3; i--) {
    const key = `${parts.slice(0, i).join('/')}/`
    if (settings[key]) {
      return settings[key]
    }
  }

  // If the URL had a port, try again without it
  if (withoutPort !== uri) {
    return pickSettingByUrl(settings, withoutPort)
  }

  return undefined
}

function removePort (parsedUrl: URL): string {
  if (parsedUrl.port === '') return parsedUrl.href
  const copy = new URL(parsedUrl.href)
  copy.port = ''
  const res = copy.toString()
  return res.endsWith('/') ? res : `${res}/`
}
