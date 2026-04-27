/// <reference path="../../../__typings__/index.d.ts"/>
import net from 'node:net'

import { afterEach, describe, expect, test } from '@jest/globals'
import { clearDispatcherCache, type DispatcherOptions, getDispatcher } from '@pnpm/network.fetch'
import { Agent, ProxyAgent } from 'undici'

afterEach(() => {
  clearDispatcherCache()
})

describe('getDispatcher', () => {
  test('returns undefined when no special options are set', () => {
    expect(getDispatcher('https://registry.npmjs.org/foo', {})).toBeUndefined()
  })

  test('returns a dispatcher when strictSsl is false', () => {
    const dispatcher = getDispatcher('https://registry.npmjs.org/foo', { strictSsl: false })
    expect(dispatcher).toBeDefined()
    expect(dispatcher).toBeInstanceOf(Agent)
  })

  test('returns a dispatcher when ca is set', () => {
    const dispatcher = getDispatcher('https://registry.npmjs.org/foo', { ca: 'test-ca' })
    expect(dispatcher).toBeDefined()
    expect(dispatcher).toBeInstanceOf(Agent)
  })

  test('returns a dispatcher when maxSockets is set', () => {
    const dispatcher = getDispatcher('https://registry.npmjs.org/foo', { maxSockets: 10 })
    expect(dispatcher).toBeDefined()
  })

  test('returns a dispatcher when localAddress is set', () => {
    const dispatcher = getDispatcher('https://registry.npmjs.org/foo', { localAddress: '127.0.0.1' })
    expect(dispatcher).toBeDefined()
  })

  test('caches dispatchers by configuration', () => {
    const opts: DispatcherOptions = { strictSsl: false }
    const d1 = getDispatcher('https://registry.npmjs.org/foo', opts)
    const d2 = getDispatcher('https://registry.npmjs.org/bar', opts)
    expect(d1).toBe(d2) // same config → same cached dispatcher
  })

  test('different maxSockets produce different dispatchers', () => {
    const d1 = getDispatcher('https://registry.npmjs.org/foo', { maxSockets: 10 })
    const d2 = getDispatcher('https://registry.npmjs.org/foo', { maxSockets: 20 })
    expect(d1).not.toBe(d2)
  })

  test('clearDispatcherCache clears cached dispatchers', () => {
    const opts: DispatcherOptions = { strictSsl: false }
    const d1 = getDispatcher('https://registry.npmjs.org/foo', opts)
    clearDispatcherCache()
    const d2 = getDispatcher('https://registry.npmjs.org/foo', opts)
    expect(d1).not.toBe(d2)
  })
})

describe('HTTP proxy', () => {
  test('returns ProxyAgent for httpProxy with http target', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'http://proxy.example.com:8080',
    })
    expect(dispatcher).toBeInstanceOf(ProxyAgent)
  })

  test('returns ProxyAgent for httpsProxy with https target', () => {
    const dispatcher = getDispatcher('https://registry.npmjs.org/foo', {
      httpsProxy: 'https://proxy.example.com:8080',
    })
    expect(dispatcher).toBeInstanceOf(ProxyAgent)
  })

  test('adds protocol prefix when proxy URL has none', () => {
    // Should not throw — the proxy URL should get protocol prepended
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'proxy.example.com:8080',
    })
    expect(dispatcher).toBeInstanceOf(ProxyAgent)
  })

  test('throws PnpmError for invalid proxy URL', () => {
    expect(() => {
      getDispatcher('http://registry.npmjs.org/foo', {
        httpProxy: 'http://[invalid',
      })
    }).toThrow(/Couldn't parse proxy URL/)
  })

  test('proxy with authentication credentials', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'http://user%21:pass%40@proxy.example.com:8080',
    })
    expect(dispatcher).toBeInstanceOf(ProxyAgent)
  })
})

describe('SOCKS proxy', () => {
  test('returns Agent (not ProxyAgent) for socks5 proxy', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'socks5://proxy.example.com:1080',
    })
    expect(dispatcher).toBeDefined()
    // SOCKS dispatcher is an Agent with custom connect, not a ProxyAgent
    expect(dispatcher).toBeInstanceOf(Agent)
    expect(dispatcher).not.toBeInstanceOf(ProxyAgent)
  })

  test('returns Agent for socks4 proxy', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'socks4://proxy.example.com:1080',
    })
    expect(dispatcher).toBeDefined()
    expect(dispatcher).toBeInstanceOf(Agent)
  })

  test('returns Agent for socks proxy with https target', () => {
    const dispatcher = getDispatcher('https://registry.npmjs.org/foo', {
      httpsProxy: 'socks5://proxy.example.com:1080',
    })
    expect(dispatcher).toBeDefined()
    expect(dispatcher).toBeInstanceOf(Agent)
  })

  test('SOCKS proxy dispatchers are cached', () => {
    const opts: DispatcherOptions = { httpProxy: 'socks5://proxy.example.com:1080' }
    const d1 = getDispatcher('http://registry.npmjs.org/foo', opts)
    const d2 = getDispatcher('http://registry.npmjs.org/bar', opts)
    expect(d1).toBe(d2)
  })

  test('SOCKS proxy can connect through a real SOCKS5 server', async () => {
    // Create a minimal SOCKS5 server that accepts connections
    const targetServer = net.createServer((socket) => {
      socket.on('data', () => {
        socket.write('HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok')
        socket.end()
      })
    })

    const socksServer = net.createServer((socket) => {
      // SOCKS5 handshake
      socket.once('data', (data) => {
        // Client greeting: version, method count, methods
        if (data[0] === 0x05) {
          // Reply: version 5, no auth required
          socket.write(Buffer.from([0x05, 0x00]))
          socket.once('data', (connectData) => {
            // Connect request: version, cmd=connect, reserved, address type, addr, port
            const port = connectData.readUInt16BE(connectData.length - 2)
            // Reply: success
            socket.write(Buffer.from([0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, (port >> 8) & 0xff, port & 0xff]))
            // Tunnel the connection to the target
            const target = net.connect(port, '127.0.0.1', () => {
              socket.pipe(target)
              target.pipe(socket)
            })
            target.on('error', () => socket.destroy())
          })
        }
      })
    })

    await new Promise<void>((resolve) => targetServer.listen(0, resolve))
    await new Promise<void>((resolve) => socksServer.listen(0, resolve))

    const targetPort = (targetServer.address() as net.AddressInfo).port
    const socksPort = (socksServer.address() as net.AddressInfo).port

    try {
      const dispatcher = getDispatcher(`http://127.0.0.1:${targetPort}/test`, {
        httpProxy: `socks5://127.0.0.1:${socksPort}`,
      })
      expect(dispatcher).toBeDefined()

      const { fetch: undiciFetch } = await import('undici')
      const res = await undiciFetch(`http://127.0.0.1:${targetPort}/test`, {
        dispatcher,
      })
      expect(res.status).toBe(200)
      expect(await res.text()).toBe('ok')
    } finally {
      targetServer.close()
      socksServer.close()
    }
  })
})

describe('noProxy', () => {
  test('bypasses proxy when noProxy matches hostname', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'http://proxy.example.com:8080',
      noProxy: 'registry.npmjs.org',
    })
    // Should return an Agent (direct), not ProxyAgent
    expect(dispatcher).toBeInstanceOf(Agent)
    expect(dispatcher).not.toBeInstanceOf(ProxyAgent)
  })

  test('bypasses proxy when noProxy matches domain suffix', () => {
    const dispatcher = getDispatcher('http://sub.npmjs.org/foo', {
      httpProxy: 'http://proxy.example.com:8080',
      noProxy: 'npmjs.org',
    })
    expect(dispatcher).toBeInstanceOf(Agent)
    expect(dispatcher).not.toBeInstanceOf(ProxyAgent)
  })

  test('does not bypass proxy when noProxy does not match', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'http://proxy.example.com:8080',
      noProxy: 'other.org',
    })
    expect(dispatcher).toBeInstanceOf(ProxyAgent)
  })

  test('bypasses proxy when noProxy is true', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'http://proxy.example.com:8080',
      noProxy: true,
    })
    expect(dispatcher).toBeInstanceOf(Agent)
    expect(dispatcher).not.toBeInstanceOf(ProxyAgent)
  })

  test('handles comma-separated noProxy list', () => {
    const dispatcher = getDispatcher('http://registry.npmjs.org/foo', {
      httpProxy: 'http://proxy.example.com:8080',
      noProxy: 'other.org, npmjs.org, example.com',
    })
    expect(dispatcher).toBeInstanceOf(Agent)
    expect(dispatcher).not.toBeInstanceOf(ProxyAgent)
  })
})

describe('client certificates', () => {
  test('picks client certificate by nerf-dart URL match', () => {
    const d1 = getDispatcher('https://registry.example.com/foo', {
      clientCertificates: {
        '//registry.example.com/': {
          ca: 'test-ca',
          cert: 'test-cert',
          key: 'test-key',
        },
      },
    })
    // Should return a dispatcher (because clientCertificates is set)
    expect(d1).toBeDefined()
  })

  test('different registries get different dispatchers with different certs', () => {
    const opts: DispatcherOptions = {
      clientCertificates: {
        '//registry.example.com/': {
          ca: 'ca-1',
          cert: 'cert-1',
          key: 'key-1',
        },
        '//other.example.com/': {
          ca: 'ca-2',
          cert: 'cert-2',
          key: 'key-2',
        },
      },
    }
    const d1 = getDispatcher('https://registry.example.com/foo', opts)
    const d2 = getDispatcher('https://other.example.com/foo', opts)
    expect(d1).not.toBe(d2) // different certs → different dispatchers
  })
})
