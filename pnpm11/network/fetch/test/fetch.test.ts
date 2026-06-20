/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, test } from '@jest/globals'
import { fetch } from '@pnpm/network.fetch'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

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
