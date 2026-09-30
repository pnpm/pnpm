import net from 'node:net'
import tls from 'node:tls'

import { PnpmError } from '@pnpm/error'
import { SocksClient } from 'socks'
import type { buildConnector } from 'undici'

import type { TlsOptions } from './dispatcher.js'

/**
 * Longest delay Node's timers accept. The socks library has no way to turn its
 * handshake timeout off, so a disabled timeout (`0`) is expressed as a delay
 * that no request outlives.
 */
const MAX_TIMER_DELAY = 2_147_483_647

export interface SocksConnectorOptions {
  proxyUrl: URL
  isHttps: boolean
  strictSsl?: boolean
  tlsConfig: TlsOptions
  timeout: number
}

export function createSocksConnector (options: SocksConnectorOptions): buildConnector.connector {
  return async (connectOpts, callback) => {
    // A timed-out handshake surfaces as an 'error' on a socket that may
    // already have been handed to undici, so every path settles at most once.
    const settle = settleOnce(callback)
    try {
      const socket = await openSocksConnection(options, connectOpts)
      if (options.isHttps) {
        upgradeToTls({ socket, hostname: connectOpts.hostname!, options, settle })
      } else {
        settle(null, socket)
      }
    } catch (err) {
      settle(err as Error, null)
    }
  }
}

function settleOnce (callback: buildConnector.Callback): buildConnector.Callback {
  let settled = false
  return (...args) => {
    if (settled) return
    settled = true
    callback(...args)
  }
}

async function openSocksConnection (options: SocksConnectorOptions, connectOpts: buildConnector.Options): Promise<net.Socket> {
  const { proxyUrl, timeout } = options
  const socksType = getSocksProxyType(proxyUrl.protocol)
  const { socket } = await SocksClient.createConnection({
    proxy: {
      host: proxyUrl.hostname,
      port: parseInt(proxyUrl.port, 10) || (socksType === 4 ? 1080 : 1080),
      type: socksType,
      userId: proxyUrl.username ? decodeURIComponent(proxyUrl.username) : undefined,
      password: proxyUrl.password ? decodeURIComponent(proxyUrl.password) : undefined,
    },
    command: 'connect',
    destination: {
      host: connectOpts.hostname,
      port: parseInt(String(connectOpts.port!), 10),
    },
    timeout: timeout === 0 ? MAX_TIMER_DELAY : timeout,
  })
  return socket as net.Socket
}

interface TlsUpgrade {
  socket: net.Socket
  hostname: string
  options: SocksConnectorOptions
  settle: buildConnector.Callback
}

function upgradeToTls ({ socket, hostname, options, settle }: TlsUpgrade): void {
  const { tlsConfig, timeout } = options
  const tlsSocket = tls.connect({
    socket,
    servername: hostname,
    ca: tlsConfig.ca,
    cert: tlsConfig.cert,
    key: tlsConfig.key,
    rejectUnauthorized: options.strictSsl ?? true,
  })
  tlsSocket.setTimeout(timeout, () => {
    tlsSocket.destroy(new PnpmError('TLS_HANDSHAKE_TIMEOUT', `The TLS handshake with ${hostname} through the SOCKS proxy timed out after ${timeout}ms`))
  })
  tlsSocket.on('secureConnect', () => {
    tlsSocket.setTimeout(0)
    settle(null, tlsSocket)
  })
  tlsSocket.on('error', (err) => {
    settle(err, null)
  })
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
