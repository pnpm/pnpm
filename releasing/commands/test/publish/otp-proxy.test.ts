import { describe, expect, it, jest } from '@jest/globals'
import type { WebAuthFetchResponse } from '@pnpm/network.web-auth'

import type { OtpContext, OtpPublishResponse } from '../../src/publish/otp.js'

// Mock @pnpm/network.fetch's fetchWithDispatcher so we can assert the polling
// request is routed through it when dispatcherOptions are supplied. This is the
// wiring that fixes https://github.com/pnpm/pnpm/issues/11561 - without it the
// doneUrl polling would bypass the proxy that the initial publish request used.
const fetchWithDispatcherMock = jest.fn<(url: string, opts: { dispatcherOptions: unknown }) => Promise<WebAuthFetchResponse>>()
const fetchMock = jest.fn()
jest.unstable_mockModule('@pnpm/network.fetch', () => ({
  fetch: fetchMock,
  fetchWithDispatcher: fetchWithDispatcherMock,
}))

const { publishWithOtpHandling } = await import('../../src/publish/otp.js')

function createOkResponse (): OtpPublishResponse {
  return { ok: true, status: 200, statusText: 'OK', text: async () => '' }
}

function createMockContext (overrides?: Partial<OtpContext>): OtpContext {
  return {
    Date: { now: () => 0 },
    setTimeout: (cb: () => void) => cb(),
    enquirer: { prompt: async () => ({ otp: '123456' }) },
    fetch: async () => {
      throw new Error('context.fetch must not be used when dispatcherOptions are provided')
    },
    globalInfo: () => {},
    globalWarn: () => {},
    publish: async () => createOkResponse(),
    process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
    ...overrides,
  }
}

describe('publishWithOtpHandling with dispatcherOptions', () => {
  const manifest = { name: 'test-pkg', version: '1.0.0' }
  const publishOptions = {} as Parameters<typeof publishWithOtpHandling>[0]['publishOptions']
  const tarballData = Buffer.from('test')
  const dispatcherOptions = { httpsProxy: 'http://proxy.example:1234' }

  it('routes doneUrl polling through fetchWithDispatcher when dispatcherOptions are provided', async () => {
    fetchWithDispatcherMock.mockResolvedValueOnce({
      headers: { get: () => null },
      json: async () => ({ token: 'web-tok' }),
      ok: true,
      status: 200,
    })
    let publishCallCount = 0
    const context = createMockContext({
      publish: async () => {
        publishCallCount++
        if (publishCallCount === 1) {
          throw Object.assign(new Error('otp'), {
            code: 'EOTP',
            body: {
              authUrl: 'https://registry.npmjs.org/auth/abc',
              doneUrl: 'https://registry.npmjs.org/auth/abc/done',
            },
          })
        }
        return createOkResponse()
      },
    })
    const result = await publishWithOtpHandling({
      context,
      dispatcherOptions,
      manifest,
      publishOptions,
      tarballData,
    })
    expect(result.ok).toBe(true)
    expect(publishCallCount).toBe(2)
    expect(fetchWithDispatcherMock).toHaveBeenCalledTimes(1)
    const callArgs = fetchWithDispatcherMock.mock.calls[0]
    expect(callArgs[0]).toBe('https://registry.npmjs.org/auth/abc/done')
    expect(callArgs[1].dispatcherOptions).toBe(dispatcherOptions)
  })
})
