import { describe, expect, it } from '@jest/globals'
import {
  type OtpContext,
  type OtpPublishResponse,
  type OtpWebAuthFetchResponse,
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  OtpWebAuthTimeoutError,
  extractUrlsFromString,
  publishWithOtpHandling,
} from '../src/otp.js'

function createOkResponse (): OtpPublishResponse {
  return { ok: true, status: 200, statusText: 'OK', text: async () => '' }
}

function createMockContext (overrides?: Partial<OtpContext>): OtpContext {
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
    globalInfo: () => {},
    process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
    publish: async () => createOkResponse(),
    ...overrides,
  }
}

describe('extractUrlsFromString', () => {
  it('extracts an HTTPS URL from a string', () => {
    const text = 'Open https://www.npmjs.com/login/abc-123-def to use your security key for authentication'
    expect([...extractUrlsFromString(text)]).toStrictEqual(['https://www.npmjs.com/login/abc-123-def'])
  })

  it('extracts an HTTP URL', () => {
    const text = 'Visit http://registry.example.com/auth/token for authentication'
    expect([...extractUrlsFromString(text)]).toStrictEqual(['http://registry.example.com/auth/token'])
  })

  it('yields nothing when no URL is present', () => {
    expect([...extractUrlsFromString('No URL here')]).toStrictEqual([])
  })

  it('extracts all URLs when multiple are present', () => {
    const text = 'Go to https://first.example.com or https://second.example.com'
    expect([...extractUrlsFromString(text)]).toStrictEqual(['https://first.example.com', 'https://second.example.com'])
  })

  it('handles URLs with path segments and query strings', () => {
    const text = 'Open https://www.npmjs.com/login/a1b2c3d4-e5f6-7890?redirect=true for auth'
    expect([...extractUrlsFromString(text)]).toStrictEqual(['https://www.npmjs.com/login/a1b2c3d4-e5f6-7890?redirect=true'])
  })
})

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
      process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
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
  })

  describe('npm-notice flow', () => {
    it('displays npm-notice messages and QR code before prompting for OTP', async () => {
      const messages: string[] = []
      let callCount = 0
      const context = createMockContext({
        publish: async (_m, _t, opts) => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), {
              code: 'EOTP',
              headers: {
                'www-authenticate': ['OTP'],
                'npm-notice': ['Open https://www.npmjs.com/login/abc-123 to use your security key for authentication'],
              },
            })
          }
          expect(opts.otp).toBe('123456')
          return createOkResponse()
        },
        globalInfo: (msg: string) => messages.push(msg),
      })
      const result = await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(result.ok).toBe(true)
      // Should have displayed the npm-notice message
      expect(messages[0]).toBe('Open https://www.npmjs.com/login/abc-123 to use your security key for authentication')
      // Should have displayed a QR code (contains block characters)
      expect(messages[1]).toContain('▄')
    })

    it('handles npm-notice without a URL (no QR code generated)', async () => {
      const messages: string[] = []
      let callCount = 0
      const context = createMockContext({
        publish: async () => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), {
              code: 'EOTP',
              headers: {
                'npm-notice': ['Please upgrade your client'],
              },
            })
          }
          return createOkResponse()
        },
        globalInfo: (msg: string) => messages.push(msg),
      })
      await publishWithOtpHandling({ context, manifest, publishOptions, tarballData })
      expect(messages).toHaveLength(1)
      expect(messages[0]).toBe('Please upgrade your client')
    })
  })

  describe('webauth flow', () => {
    it('polls doneUrl and uses returned token', async () => {
      let publishCallCount = 0
      let fetchCallCount = 0
      const context = createMockContext({
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
        fetch: async (): Promise<OtpWebAuthFetchResponse> => {
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
    })

    it('respects Retry-After header when polling', async () => {
      const setTimeoutDelays: number[] = []
      let fetchCallCount = 0
      const context = createMockContext({
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
        fetch: async (): Promise<OtpWebAuthFetchResponse> => {
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
    })

    it('throws OtpWebAuthTimeoutError after 5 minutes', async () => {
      let time = 0
      const context = createMockContext({
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
        fetch: async (): Promise<OtpWebAuthFetchResponse> => ({
          headers: { get: () => null },
          json: async () => ({}),
          ok: true,
          status: 202,
        }),
      })
      await expect(publishWithOtpHandling({ context, manifest, publishOptions, tarballData }))
        .rejects.toBeInstanceOf(OtpWebAuthTimeoutError)
    })
  })
})
