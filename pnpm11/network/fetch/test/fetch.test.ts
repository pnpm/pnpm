/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import https from 'node:https'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { afterEach, describe, expect, jest, test } from '@jest/globals'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { clearDispatcherCache, fetch, isNonRetryableError } from '@pnpm/network.fetch'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

import { startServer } from './utils/trickleServer.js'

const CERTS_DIR = path.join(import.meta.dirname, '__certs__')

test('metadata retry logs redact signed URL parameters', async () => {
  const originalDispatcher = getGlobalDispatcher()
  const mockAgent = new MockAgent()
  mockAgent.disableNetConnect()
  setGlobalDispatcher(mockAgent)
  const log = jest.spyOn(requestRetryLogger, 'debug')
  try {
    const pool = mockAgent.get('https://registry.example')
    pool.intercept({ path: '/metadata?token=secret', method: 'GET' }).reply(503, 'Unavailable')
    pool.intercept({ path: '/metadata?token=secret', method: 'GET' }).reply(200, '{}')
    const response = await fetch('https://registry.example/metadata?token=secret', {
      retry: { retries: 1, minTimeout: 1, maxTimeout: 1 },
    })
    expect(response.status).toBe(200)
    expect(log).toHaveBeenCalledWith(expect.objectContaining({ url: 'https://registry.example/metadata' }))
    expect(JSON.stringify(log.mock.calls)).not.toContain('token=secret')
  } finally {
    log.mockRestore()
    await mockAgent.close()
    setGlobalDispatcher(originalDispatcher)
  }
})

test('metadata retry logs redact request URLs echoed by transport errors', async () => {
  const log = jest.spyOn(requestRetryLogger, 'debug')
  try {
    await expect(fetch('https://user:password@registry.example/metadata?token=secret#fragment', {
      retry: { retries: 1, minTimeout: 1, maxTimeout: 1 },
    })).rejects.toThrow('Request cannot be constructed from a URL that includes credentials')
    expect(log).toHaveBeenCalledWith(expect.objectContaining({ url: 'https://registry.example/metadata' }))
    for (const secret of ['password', 'token=secret', 'fragment']) {
      expect(JSON.stringify(log.mock.calls)).not.toContain(secret)
    }
  } finally {
    log.mockRestore()
  }
})

test('fetch rejects, and does not hang, on a non-retryable error code', async () => {
  const originalDispatcher: Dispatcher = getGlobalDispatcher()
  const mockAgent = new MockAgent()
  mockAgent.disableNetConnect()
  setGlobalDispatcher(mockAgent)
  try {
    const tlsError = Object.assign(
      new Error('self signed certificate in certificate chain'),
      { code: 'SELF_SIGNED_CERT_IN_CHAIN' }
    )
    mockAgent
      .get('http://registry.pnpm.io')
      .intercept({ path: '/is-positive', method: 'GET' })
      .replyWithError(tlsError)

    const TIMEOUT = Symbol('timeout')
    let timer: NodeJS.Timeout | undefined
    const outcome = await Promise.race([
      fetch('http://registry.pnpm.io/is-positive', { retry: { retries: 0 } })
        .then(() => 'resolved', (err: unknown) => err),
      new Promise<typeof TIMEOUT>((resolve) => {
        timer = setTimeout(() => resolve(TIMEOUT), 2000)
      }),
    ])
    if (timer) clearTimeout(timer)

    expect(outcome).not.toBe(TIMEOUT)
    expect(outcome).not.toBe('resolved')
    const err = outcome as Error & { code?: string, cause?: { code?: string } }
    expect(err.code ?? err.cause?.code).toBe('SELF_SIGNED_CERT_IN_CHAIN')
  } finally {
    await mockAgent.close()
    setGlobalDispatcher(originalDispatcher)
  }
})

// https://github.com/pnpm/pnpm/issues/9134
test.each([
  'CERT_CHAIN_TOO_LONG',
  'CERT_HAS_EXPIRED',
  'DEPTH_ZERO_SELF_SIGNED_CERT',
])('fetch does not retry a request that fails with %s', async (code) => {
  const originalDispatcher = getGlobalDispatcher()
  const mockAgent = new MockAgent()
  mockAgent.disableNetConnect()
  setGlobalDispatcher(mockAgent)
  try {
    mockAgent.get('https://registry.example')
      .intercept({ path: '/is-positive', method: 'GET' })
      .replyWithError(Object.assign(new Error(code), { code }))
      .times(2)

    const err = await fetch('https://registry.example/is-positive', {
      retry: { retries: 1, minTimeout: 1, maxTimeout: 1 },
    }).then(() => undefined, (rejection: unknown) => rejection)

    expect(err).toHaveProperty('code', code)
    expect(mockAgent.pendingInterceptors()).toHaveLength(1)
  } finally {
    await mockAgent.close()
    setGlobalDispatcher(originalDispatcher)
  }
})

test('wrapped non-retryable errors keep their cause code', () => {
  const certificate = Object.assign(new Error('certificate has expired'), { code: 'CERT_HAS_EXPIRED' })
  const diskFull = Object.assign(new Error('no space left on device'), { code: 'ENOSPC' })
  expect(isNonRetryableError(Object.assign(new Error('fetch failed', { cause: certificate }), { code: 'FETCH_FAILED' }))).toBe(true)
  expect(isNonRetryableError(Object.assign(new Error('store failed', { cause: diskFull }), { code: 'ERR_PNPM_TARBALL_EXTRACT' }))).toBe(true)
  expect(isNonRetryableError(Object.assign(new Error('store write failed'), { code: 'ERR_PNPM_ENOSPC' }))).toBe(true)
  expect(isNonRetryableError(Object.assign(new Error('socket hang up'), { code: 'ECONNRESET' }))).toBe(false)
})

// https://github.com/pnpm/pnpm/issues/9134
test('fetch does not retry a request to a server with an untrusted certificate', async () => {
  const server = https.createServer({
    key: fs.readFileSync(path.join(CERTS_DIR, 'server-key.pem')),
    cert: fs.readFileSync(path.join(CERTS_DIR, 'server-crt.pem')),
  })
  let connections = 0
  server.on('connection', () => {
    connections++
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  const { port } = server.address() as AddressInfo
  try {
    const err = await fetch(`https://localhost:${port}/is-positive`, {
      retry: { retries: 2, minTimeout: 1, maxTimeout: 1 },
    }).then(() => undefined, (error: unknown) => error)
    expect(err).toHaveProperty('code', 'UNABLE_TO_VERIFY_LEAF_SIGNATURE')
    expect(connections).toBe(1)
  } finally {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
  }
})

// https://github.com/pnpm/pnpm/issues/14604
describe('the fetch timeout measures inactivity, not total time', () => {
  const CHUNK = 'chunk'
  const CHUNKS = 6
  const CHUNK_INTERVAL = 60
  const TIMEOUT = 300

  afterEach(() => {
    clearDispatcherCache()
  })

  test('a body that keeps arriving is read to the end', async () => {
    await using server = await startServer((res) => {
      let sent = 0
      const writeChunk = (): void => {
        res.write(CHUNK)
        if (++sent < CHUNKS) {
          setTimeout(writeChunk, CHUNK_INTERVAL)
        } else {
          res.end()
        }
      }
      setTimeout(writeChunk, CHUNK_INTERVAL)
    })

    const response = await fetch(server.url, { timeout: TIMEOUT, retry: { retries: 0 } })

    await expect(response.text()).resolves.toBe(CHUNK.repeat(CHUNKS))
  })

  test('a body that stops arriving fails', async () => {
    await using server = await startServer((res) => {
      res.write(CHUNK)
    })

    const response = await fetch(server.url, { timeout: TIMEOUT, retry: { retries: 0 } })

    await expect(response.text()).rejects.toMatchObject({
      cause: expect.objectContaining({ code: 'UND_ERR_BODY_TIMEOUT' }),
    })
  })
})

