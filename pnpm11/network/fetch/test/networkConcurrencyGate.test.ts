/// <reference path="../../../__typings__/index.d.ts"/>
import { createServer } from 'node:http'
import type { AddressInfo } from 'node:net'

import { afterEach, expect, test } from '@jest/globals'
import { clearDispatcherCache, createFetchFromRegistry } from '@pnpm/network.fetch'

import { createNetworkConcurrencyGate } from '../src/networkConcurrencyGate.js'

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
