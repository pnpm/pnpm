import { describe, expect, it, jest } from '@jest/globals'
import {
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  type WebAuthFetchResponse,
  WebAuthTimeoutError,
} from '@pnpm/network.web-auth'

import {
  type OtpContext,
  type OtpPublishResponse,
  publishWithOtpHandling,
} from '../../src/publish/otp.js'

function createOkResponse (): OtpPublishResponse {
  return { ok: true, status: 200, statusText: 'OK', text: async () => '' }
}

type MockContextOverrides = Omit<Partial<OtpContext>, 'process'> & {
  process?: Partial<OtpContext['process']>
}

function createMockContext (overrides?: MockContextOverrides): OtpContext {
  return {
    Date: { now: () => 0 },
    setTimeout: (cb: () => void) => cb(),
    enquirer: { prompt: async () => ({ otp: '123456' }) },
    fetch: async () => ({
      headers: { get: () => null },
      json: async () => ({}),
      ok: false,
      status: 404,
    }),
    globalInfo: msg => {
      throw new Error(`Unexpected call to globalInfo: ${msg}`)
    },
    globalWarn: msg => {
      throw new Error(`Unexpected call to globalWarn: ${msg}`)
    },
    publish: async () => createOkResponse(),
    ...overrides,
    process: {
      stdin: { isTTY: true },
      stdout: { isTTY: true },
      ...overrides?.process,
    },
  }
}

describe('publishWithOtpHandling', () => {
  const manifest = { name: 'test-pkg', version: '1.0.0' }
  const publishOptions = {} as Parameters<typeof publishWithOtpHandling>[0]['publishOptions']
  const tarballData = Buffer.from('test')

  it('returns response when publish succeeds without OTP', async () => {
    const response = createOkResponse()
    const context = createMockContext({
      publish: async () => response,
    })
    const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
    expect(result).toBe(response)
  })

  it('throws non-OTP errors as-is', async () => {
    const error = new Error('network error')
    const context = createMockContext({
      publish: async () => {
        throw error
      },
    })
    await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
      .rejects.toBe(error)
  })

  it('throws OtpNonInteractiveError when terminal is not interactive', async () => {
    const context = createMockContext({
      process: { stdin: { isTTY: false } },
      publish: async () => {
        throw Object.assign(new Error('otp'), { code: 'EOTP' })
      },
    })
    await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
      .rejects.toBeInstanceOf(OtpNonInteractiveError)
  })

  describe('classic OTP flow', () => {
    it('prompts for OTP and retries publish', async () => {
      let callCount = 0
      const context = createMockContext({
        publish: async (_m, _t, opts) => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), { code: 'EOTP' })
          }
          expect(opts.otp).toBe('654321')
          return createOkResponse()
        },
        enquirer: { prompt: async () => ({ otp: '654321' }) },
      })
      const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(result.ok).toBe(true)
      expect(callCount).toBe(2)
    })

    it('throws OtpSecondChallengeError if retry also requires OTP', async () => {
      const context = createMockContext({
        publish: async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
      })
      await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
        .rejects.toBeInstanceOf(OtpSecondChallengeError)
    })

    it('throws non-OTP errors from the retry publish as-is', async () => {
      let callCount = 0
      const retryError = new Error('server error')
      const context = createMockContext({
        publish: async () => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), { code: 'EOTP' })
          }
          throw retryError
        },
      })
      await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
        .rejects.toBe(retryError)
    })

    it('re-throws the original OTP error when enquirer returns no OTP', async () => {
      const context = createMockContext({
        publish: async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
        enquirer: { prompt: async () => ({ otp: '' }) },
      })
      await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
        .rejects.toMatchObject({ code: 'EOTP' })
    })

    it('re-throws the original OTP error when enquirer returns undefined', async () => {
      const context = createMockContext({
        publish: async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
        enquirer: { prompt: async () => undefined },
      })
      await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
        .rejects.toMatchObject({ code: 'EOTP' })
    })
  })

  describe('webauth flow', () => {
    it('polls doneUrl and uses returned token', async () => {
      let publishCallCount = 0
      let fetchCallCount = 0
      const globalInfo = jest.fn()
      const context = createMockContext({
        globalInfo,
        publish: async (_m, _t, opts) => {
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
          expect(opts.otp).toBe('web-token-123')
          return createOkResponse()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => {
          fetchCallCount++
          if (fetchCallCount < 3) {
            return {
              headers: { get: () => '1' },
              json: async () => ({}),
              ok: true,
              status: 202,
            }
          }
          return {
            headers: { get: () => null },
            json: async () => ({ token: 'web-token-123' }),
            ok: true,
            status: 200,
          }
        },
      })
      const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(result.ok).toBe(true)
      expect(publishCallCount).toBe(2)
      expect(fetchCallCount).toBe(3)
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })

    it('respects Retry-After header when polling', async () => {
      const setTimeoutDelays: number[] = []
      let fetchCallCount = 0
      const globalInfo = jest.fn()
      const context = createMockContext({
        globalInfo,
        publish: async () => {
          if (fetchCallCount === 0) {
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
        setTimeout: (cb: () => void, ms: number) => {
          setTimeoutDelays.push(ms)
          cb()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => {
          fetchCallCount++
          if (fetchCallCount === 1) {
            return {
              headers: { get: (name: string) => name === 'retry-after' ? '5' : null },
              json: async () => ({}),
              ok: true,
              status: 202,
            }
          }
          return {
            headers: { get: () => null },
            json: async () => ({ token: 'tok' }),
            ok: true,
            status: 200,
          }
        },
      })
      await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      // First setTimeout is the default 1s poll interval,
      // second is the additional delay (5s Retry-After minus the 1s already waited),
      // third is the default 1s poll interval for the next iteration.
      expect(setTimeoutDelays).toStrictEqual([1000, 4000, 1000])
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })

    it('continues polling when fetch throws', async () => {
      let publishCallCount = 0
      let fetchCallCount = 0
      const globalInfo = jest.fn()
      const context = createMockContext({
        globalInfo,
        publish: async (_m, _t, opts) => {
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
          expect(opts.otp).toBe('tok')
          return createOkResponse()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => {
          fetchCallCount++
          if (fetchCallCount === 1) {
            throw new Error('network failure')
          }
          return {
            headers: { get: () => null },
            json: async () => ({ token: 'tok' }),
            ok: true,
            status: 200,
          }
        },
      })
      const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(result.ok).toBe(true)
      expect(fetchCallCount).toBe(2)
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })

    it('continues polling when response is not ok', async () => {
      let publishCallCount = 0
      let fetchCallCount = 0
      const globalInfo = jest.fn()
      const context = createMockContext({
        globalInfo,
        publish: async (_m, _t, opts) => {
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
          expect(opts.otp).toBe('tok')
          return createOkResponse()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => {
          fetchCallCount++
          if (fetchCallCount === 1) {
            return {
              headers: { get: () => null },
              json: async () => ({}),
              ok: false,
              status: 404,
            }
          }
          return {
            headers: { get: () => null },
            json: async () => ({ token: 'tok' }),
            ok: true,
            status: 200,
          }
        },
      })
      const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(result.ok).toBe(true)
      expect(fetchCallCount).toBe(2)
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })

    it('continues polling when response.json() throws', async () => {
      let publishCallCount = 0
      let fetchCallCount = 0
      const globalInfo = jest.fn()
      const context = createMockContext({
        globalInfo,
        publish: async (_m, _t, opts) => {
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
          expect(opts.otp).toBe('tok')
          return createOkResponse()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => {
          fetchCallCount++
          if (fetchCallCount === 1) {
            return {
              headers: { get: () => null },
              json: async () => {
                throw new Error('invalid json')
              },
              ok: true,
              status: 200,
            }
          }
          return {
            headers: { get: () => null },
            json: async () => ({ token: 'tok' }),
            ok: true,
            status: 200,
          }
        },
      })
      const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(result.ok).toBe(true)
      expect(fetchCallCount).toBe(2)
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })

    it('throws WebAuthTimeoutError after 5 minutes', async () => {
      let time = 0
      const globalInfo = jest.fn()
      const context = createMockContext({
        globalInfo,
        Date: { now: () => time },
        publish: async () => {
          throw Object.assign(new Error('otp'), {
            code: 'EOTP',
            body: {
              authUrl: 'https://registry.npmjs.org/auth/abc',
              doneUrl: 'https://registry.npmjs.org/auth/abc/done',
            },
          })
        },
        setTimeout: (cb: () => void) => {
          time += 6 * 60 * 1000 // Jump past timeout
          cb()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => ({
          headers: { get: () => null },
          json: async () => ({}),
          ok: true,
          status: 202,
        }),
      })
      await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
        .rejects.toBeInstanceOf(WebAuthTimeoutError)
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })
  })
})
