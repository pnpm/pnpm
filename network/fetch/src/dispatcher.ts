import net from 'node:net'
import tls from 'node:tls'
import { URL } from 'node:url'

import { nerfDart } from '@pnpm/config.nerf-dart'
import { PnpmError } from '@pnpm/error'
import type { SslConfig } from '@pnpm/types'
import { LRUCache } from 'lru-cache'
import { SocksClient } from 'socks'
import { Agent, type Dispatcher, ProxyAgent } from 'undici'

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

  const parsedUri = new URL(uri)

  if ((opts.httpProxy || opts.httpsProxy) && !checkNoProxy(parsedUri, opts)) {
    const proxyDispatcher = getProxyDispatcher(parsedUri, opts)
    if (proxyDispatcher) return proxyDispatcher
  }
  return getNonProxyDispatcher(parsedUri, opts)
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
    opts.clientCertificates ||
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

function proxyAuthFingerprint (proxyUrl: URL): string {
  if (!proxyUrl.username) return 'no-auth'
  // Include auth presence and lengths in the cache key without storing credentials.
  // Different credentials for the same proxy host is not a realistic scenario,
  // so this is sufficient for cache key uniqueness.
  return `auth:${proxyUrl.username.length}:${proxyUrl.password.length}`
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
    `proxy:${proxyUrl.protocol}//${proxyUrl.host}:${proxyUrl.port}:${proxyAuthFingerprint(proxyUrl)}`,
    `https:${isHttps.toString()}`,
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `max-sockets:${(opts.maxSockets ?? DEFAULT_MAX_SOCKETS).toString()}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && ca?.toString()) || '>no-ca<'}`,
    `cert:${(isHttps && cert?.toString()) || '>no-cert<'}`,
    `key:${(isHttps && certKey?.toString()) || '>no-key<'}`,
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

  return new Agent({
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    connect: async (connectOpts, callback) => {
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
          tlsSocket.on('secureConnect', () => {
            callback(null, tlsSocket)
          })
          tlsSocket.on('error', (err) => {
            callback(err, null)
          })
        } else {
          callback(null, socket as net.Socket)
        }
      } catch (err) {
        callback(err as Error, null)
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
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `max-sockets:${(opts.maxSockets ?? DEFAULT_MAX_SOCKETS).toString()}`,
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

  const agent = new Agent({
    connections: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    connectTimeout,
    keepAliveTimeout: 4000,
    keepAliveMaxTimeout: 15000,
    connect: isHttps
      ? {
        ca,
        cert,
        key: certKey,
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
