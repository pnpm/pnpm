/// <reference path="../../../__typings__/index.d.ts"/>
import { afterEach, describe, expect, jest, test } from '@jest/globals'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { clearDispatcherCache, fetch } from '@pnpm/network.fetch'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

import { startServer } from './utils/trickleServer.js'

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

