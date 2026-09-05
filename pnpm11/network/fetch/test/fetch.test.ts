/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, jest, test } from '@jest/globals'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { fetch } from '@pnpm/network.fetch'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

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
