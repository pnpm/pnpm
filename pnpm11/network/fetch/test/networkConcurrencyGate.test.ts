/// <reference path="../../../__typings__/index.d.ts"/>
import { createServer } from 'node:http'
import type { AddressInfo } from 'node:net'

import { afterEach, expect, test } from '@jest/globals'
import { clearDispatcherCache, createFetchFromRegistry } from '@pnpm/network.fetch'

import { getConnectionOrigin } from '../src/dispatcher.js'
import { createNetworkConcurrencyGate, createOriginConcurrencyGates } from '../src/networkConcurrencyGate.js'

test('a timeout with peers in flight shrinks the cap and holds new acquires', async () => {
  const gate = createNetworkConcurrencyGate(3)
  await gate.acquire()
  await gate.acquire()
  await gate.acquire()
  expect(gate.downscaleIfPeersActive()).toBe(true)
  expect(gate.limit).toBe(1)
  expect(gate.downscaleIfPeersActive()).toBe(false)

  let granted = false
  const waiting = gate.acquire().then(() => {
    granted = true
  })
  gate.release()
  gate.release()
  await Promise.resolve()
  expect(granted).toBe(false)

  gate.release()
  await waiting
  expect(granted).toBe(true)
})

test('a lone in-flight request does not shrink the cap', async () => {
  const gate = createNetworkConcurrencyGate(4)
  await gate.acquire()
  expect(gate.downscaleIfPeersActive()).toBe(false)
  expect(gate.limit).toBe(4)
  gate.release()
})

test('a downscale on one origin leaves other origins unbounded', async () => {
  const gateFor = createOriginConcurrencyGates()
  const slow = gateFor('https://slow.example')
  expect(gateFor('https://slow.example')).toBe(slow)
  await slow.acquire()
  await slow.acquire()
  expect(slow.downscaleIfPeersActive()).toBe(true)

  const other = gateFor('https://fast.example')
  await other.acquire()
  await other.acquire()
  await other.acquire()
  expect(other.limit).toBe(Number.POSITIVE_INFINITY)
})

test('requests through one proxy share its origin unless noProxy exempts them', () => {
  const opts = { httpsProxy: 'http://user:secret@proxy.example:8080', noProxy: 'direct.example' }
  expect(getConnectionOrigin(new URL('https://registry.example/pkg'), opts)).toBe('http://proxy.example:8080')
  expect(getConnectionOrigin(new URL('https://cdn.example/pkg.tgz'), opts)).toBe('http://proxy.example:8080')
  expect(getConnectionOrigin(new URL('https://direct.example/pkg'), opts)).toBe('https://direct.example')
  expect(getConnectionOrigin(new URL('http://plain.example/pkg'), opts)).toBe('http://plain.example')
})

test('distinct SOCKS proxies get distinct connection origins without their credentials', () => {
  const uri = new URL('https://registry.example/pkg')
  expect(getConnectionOrigin(uri, { httpsProxy: 'socks5://user:secret@one.example:1080' })).toBe('socks5://one.example:1080')
  expect(getConnectionOrigin(uri, { httpsProxy: 'socks5://two.example:1080' })).toBe('socks5://two.example:1080')
})

afterEach(() => {
  clearDispatcherCache()
})

test('a fetch timeout while another download is active retries at one connection', async () => {
  let open = 0
  let accepted = 0
  const server = createServer((req, res) => {
    accepted++
    open++
    const stall = accepted <= 2
    let closed = false
    req.on('close', () => {
      if (closed) return
      closed = true
      open--
    })
    if (stall) return
    const timer = setInterval(() => {
      if (!closed && open === 1) {
        clearInterval(timer)
        res.end('ok')
      }
    }, 10)
    req.on('close', () => {
      clearInterval(timer)
    })
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  const { port } = server.address() as AddressInfo
  const url = `http://127.0.0.1:${port}/pkg`
  try {
    const fetchFromRegistry = createFetchFromRegistry({})
    const read = async () => {
      const response = await fetchFromRegistry(url, {
        retry: { factor: 1, maxTimeout: 20, minTimeout: 1, retries: 4 },
        timeout: 200,
      })
      return response.text()
    }
    const [left, right] = await Promise.all([read(), read()])
    expect(left).toBe('ok')
    expect(right).toBe('ok')
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    })
  }
}, 20_000)
