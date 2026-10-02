import { URL } from 'node:url'

import { nerfDart } from '@pnpm/config.registry-auth-key'
import { PnpmError } from '@pnpm/error'
import type { TlsConfig } from '@pnpm/types'
import { LRUCache } from 'lru-cache'
import { Agent, type Dispatcher, getGlobalDispatcher, ProxyAgent, setGlobalDispatcher } from 'undici'

import { createSocksConnector } from './socksConnector.js'

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
  return (opts, handler) => dispatch(withoutSecFetchHeaders(opts), handler)
}

type HeaderValue = string | string[] | undefined

function withoutSecFetchHeaders (opts: Dispatcher.DispatchOptions): Dispatcher.DispatchOptions {
  if (!opts.headers) return opts
  if (Array.isArray(opts.headers)) {
    return { ...opts, headers: filterFlatHeaders(opts.headers) }
  }
  if (typeof opts.headers === 'object') {
    return { ...opts, headers: filterHeaderEntries(opts.headers) }
  }
  return opts
}

// Flat array format: [key1, val1, key2, val2, ...]
function filterFlatHeaders (headers: string[]): string[] {
  const filtered: string[] = []
  for (let nameIndex = 0; nameIndex < headers.length; nameIndex += 2) {
    if (!isSecFetchHeader(headers[nameIndex])) {
      filtered.push(headers[nameIndex], headers[nameIndex + 1])
    }
  }
  return filtered
}

function filterHeaderEntries (headers: object): Record<string, HeaderValue> {
  // undici also accepts an iterable of [key, value] pairs (e.g. a Map or
  // web Headers). Use that iterator when present; otherwise fall back to
  // Object.entries for plain IncomingHttpHeaders objects.
  const entries = Symbol.iterator in headers
    ? (headers as Iterable<[string, HeaderValue]>)
    : Object.entries(headers as Record<string, HeaderValue>)
  const filtered: Record<string, HeaderValue> = {}
  for (const [key, value] of entries) {
    if (!isSecFetchHeader(key)) {
      filtered[key] = value
    }
  }
  return filtered
}

function isSecFetchHeader (headerName: string): boolean {
  return headerName.toLowerCase().startsWith('sec-fetch-')
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

/**
 * The origin a request to `uri` opens its connection to: the proxy's when
 * one applies, so requests sharing a proxy share its origin.
 */
export function getConnectionOrigin (uri: URL, opts: DispatcherOptions): string {
  const proxy = uri.protocol === 'https:' ? opts.httpsProxy : opts.httpProxy
  if (!proxy || checkNoProxy(uri, opts)) return uri.origin
  const proxyUrl = parseProxyUrl(proxy, uri.protocol)
  // `URL.origin` is "null" for non-special schemes such as socks5:.
  return `${proxyUrl.protocol}//${proxyUrl.host}`
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


export type TlsOptions = Pick<DispatcherOptions, 'ca' | 'cert' | 'key'>

function getProxyDispatcher (parsedUri: URL, opts: DispatcherOptions): Dispatcher | null {
  const isHttps = parsedUri.protocol === 'https:'
  const proxy = isHttps ? opts.httpsProxy : opts.httpProxy

  if (!proxy) return null

  const proxyUrl = parseProxyUrl(proxy, parsedUri.protocol)
  const tlsConfig = pickTlsConfig(parsedUri, opts)

  const key = [
    `proxy:${proxyUrl.protocol}//${proxyUrl.username}:${proxyUrl.password}@${proxyUrl.host}:${proxyUrl.port}`,
    ...getDispatcherKeyParts(isHttps, opts, tlsConfig),
  ].join(':')

  return getCachedDispatcher(key, () => proxyUrl.protocol.startsWith('socks')
    ? createSocksDispatcher(proxyUrl, parsedUri, opts, tlsConfig)
    : createHttpProxyDispatcher(proxyUrl, isHttps, opts, tlsConfig)
  )
}

function pickTlsConfig (parsedUri: URL, opts: DispatcherOptions): TlsOptions {
  const sslConfig = pickSettingByUrl(opts.clientCertificates, parsedUri.href)
  const { ca, cert, key } = { ...opts, ...sslConfig }
  return { ca, cert, key }
}

function getDispatcherKeyParts (isHttps: boolean, opts: DispatcherOptions, tlsConfig: TlsOptions): string[] {
  return [
    `https:${isHttps.toString()}`,
    `timeout:${inactivityTimeout(opts).toString()}`,
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `max-sockets:${(opts.maxSockets ?? DEFAULT_MAX_SOCKETS).toString()}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && tlsConfig.ca?.toString()) || '-'}`,
    `cert:${(isHttps && tlsConfig.cert?.toString()) || '-'}`,
    `key:${(isHttps && tlsConfig.key?.toString()) || '-'}`,
  ]
}

function getCachedDispatcher (key: string, createDispatcher: () => Dispatcher): Dispatcher {
  if (DISPATCHER_CACHE.has(key)) {
    return DISPATCHER_CACHE.get(key)!
  }
  const dispatcher = createDispatcher().compose(stripSecFetchHeaders)
  DISPATCHER_CACHE.set(key, dispatcher)
  return dispatcher
}

function createHttpProxyDispatcher (
  proxyUrl: URL,
  isHttps: boolean,
  opts: DispatcherOptions,
  tlsConfig: TlsOptions
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
  tlsConfig: TlsOptions
): Dispatcher {
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
    connect: createSocksConnector({
      proxyUrl,
      isHttps: targetUri.protocol === 'https:',
      strictSsl: opts.strictSsl,
      tlsConfig,
      timeout,
    }),
  })
}

function getNonProxyDispatcher (parsedUri: URL, opts: DispatcherOptions): Dispatcher {
  const isHttps = parsedUri.protocol === 'https:'
  const tlsConfig = pickTlsConfig(parsedUri, opts)
  const key = getDispatcherKeyParts(isHttps, opts, tlsConfig).join(':')
  return getCachedDispatcher(key, () => createDirectAgent(isHttps, opts, tlsConfig))
}

function createDirectAgent (isHttps: boolean, opts: DispatcherOptions, tlsConfig: TlsOptions): Dispatcher {
  const timeout = inactivityTimeout(opts)
  return new Agent({
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    connectTimeout: timeout,
    headersTimeout: timeout,
    bodyTimeout: timeout,
    keepAliveTimeout: KEEP_ALIVE_TIMEOUT,
    keepAliveMaxTimeout: KEEP_ALIVE_MAX_TIMEOUT,
    connect: isHttps
      ? {
        autoSelectFamily: true,
        ca: tlsConfig.ca,
        cert: tlsConfig.cert,
        key: tlsConfig.key,
        rejectUnauthorized: opts.strictSsl ?? true,
        localAddress: opts.localAddress,
      }
      : {
        autoSelectFamily: true,
        localAddress: opts.localAddress,
      },
  })
}

function checkNoProxy (parsedUri: URL, opts: { noProxy?: boolean | string }): boolean {
  const host = toReversedLabels(parsedUri.hostname)
  if (typeof opts.noProxy === 'string') {
    const noproxyArr = opts.noProxy.split(',').map(entry => entry.trim())
    return noproxyArr.some(entry => isHostCoveredByNoProxyEntry(host, entry))
  }
  return opts.noProxy === true
}

function toReversedLabels (hostname: string): string[] {
  return hostname
    .split('.')
    .filter(label => label)
    .reverse()
}

function isHostCoveredByNoProxyEntry (hostLabels: string[], noProxyEntry: string): boolean {
  const noParts = toReversedLabels(noProxyEntry)
  if (noParts.length === 0) {
    return false
  }
  return noParts.every((label, labelIndex) => hostLabels[labelIndex] === label)
}

/**
 * Pick SSL/TLS configuration by URL using nerf-dart matching.
 * This matches the behavior of @pnpm/network.config's pickSettingByUrl.
 */
function pickSettingByUrl<Setting> (
  settings: Record<string, Setting> | undefined,
  uri: string
): Setting | undefined {
  if (!settings) return undefined

  if (settings[uri]) return settings[uri]

  // Use nerf-dart format for matching (e.g., //registry.npmjs.org/)
  const nerf = nerfDart(uri)
  if (settings[nerf]) return settings[nerf]

  const parsedUrl = new URL(uri)
  const withoutPort = removePort(parsedUrl)
  if (settings[withoutPort]) return settings[withoutPort]

  const byParentPath = pickSettingByNerfDartParentPath(settings, nerf)
  if (byParentPath) return byParentPath

  if (withoutPort !== uri) {
    return pickSettingByUrl(settings, withoutPort)
  }

  return undefined
}

function pickSettingByNerfDartParentPath<Setting> (
  settings: Record<string, Setting>,
  nerf: string
): Setting | undefined {
  const maxParts = Object.keys(settings).reduce((max, key) => {
    const parts = key.split('/').length
    return parts > max ? parts : max
  }, 0)
  const parts = nerf.split('/')
  for (let partCount = Math.min(parts.length, maxParts) - 1; partCount >= 3; partCount--) {
    const key = `${parts.slice(0, partCount).join('/')}/`
    if (settings[key]) {
      return settings[key]
    }
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
