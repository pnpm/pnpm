import { describe, expect, it, jest } from '@jest/globals'
import {
  type OtpContext,
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  SyntheticOtpError,
  type WebAuthFetchOptions,
  type WebAuthFetchResponse,
  WebAuthTimeoutError,
  withOtpHandling,
} from '@pnpm/network.web-auth'

function createMockResponse (init: {
  ok: boolean
  status: number
  json?: unknown
  headers?: WebAuthFetchResponse['headers']
}): WebAuthFetchResponse {
  let bodyConsumed = false
  return {
    ok: init.ok,
    status: init.status,
    json: async () => {
      if (bodyConsumed) throw new Error('Unexpected double consumption of response body')
      bodyConsumed = true
      return init.json ?? {}
    },
    headers: init.headers ?? {
      get: name => {
        throw new Error(`Unexpected call to headers.get: ${name}`)
      },
    },
  }
}

type MockContextOverrides = Omit<Partial<OtpContext>, 'process'> & {
  process?: Partial<OtpContext['process']>
}

const createOtpMockContext = (overrides?: MockContextOverrides): OtpContext => ({
  Date: { now: () => 0 },
  setTimeout: (cb: () => void) => cb(),
  enquirer: { prompt: async () => ({ otp: '123456' }) },
  fetch: async () => createMockResponse({
    ok: false,
    status: 404,
  }),
  globalInfo: msg => {
    throw new Error(`Unexpected call to globalInfo: ${msg}`)
  },
  globalWarn: msg => {
    throw new Error(`Unexpected call to globalWarn: ${msg}`)
  },
  ...overrides,
  process: {
    stdin: { isTTY: true },
    stdout: { isTTY: true },
    ...overrides?.process,
  },
})

const fetchOptions: WebAuthFetchOptions = { method: 'GET' }

describe('withOtpHandling', () => {
  it('returns the result when the operation succeeds without OTP', async () => {
    const context = createOtpMockContext()
    const result = await withOtpHandling({ context, fetchOptions, operation: async () => 'success' })
    expect(result).toBe('success')
  })

  it('throws non-OTP errors as-is', async () => {
    const error = new Error('network error')
    const context = createOtpMockContext()
    await expect(withOtpHandling({ context, fetchOptions, operation: async () => {
      throw error
    } }))
      .rejects.toBe(error)
  })

  it('throws OtpNonInteractiveError when terminal is not interactive', async () => {
    const context = createOtpMockContext({
      process: { stdin: { isTTY: false } },
    })
    const operation = async () => {
      throw Object.assign(new Error('otp'), { code: 'EOTP' })
    }
    await expect(withOtpHandling({ context, fetchOptions, operation }))
      .rejects.toBeInstanceOf(OtpNonInteractiveError)
  })

  it('throws OtpNonInteractiveError when stdout is not interactive', async () => {
    const context = createOtpMockContext({
      process: { stdout: { isTTY: false } },
    })
    const operation = async () => {
      throw Object.assign(new Error('otp'), { code: 'EOTP' })
    }
    await expect(withOtpHandling({ context, fetchOptions, operation }))
      .rejects.toBeInstanceOf(OtpNonInteractiveError)
  })

  describe('classic OTP flow', () => {
    it('prompts for OTP and retries operation', async () => {
      let callCount = 0
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: '654321' }) },
      })
      const result = await withOtpHandling({
        context,
        fetchOptions,
        operation: async otp => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), { code: 'EOTP' })
          }
          expect(otp).toBe('654321')
          return 'ok'
        },
      })
      expect(result).toBe('ok')
      expect(callCount).toBe(2)
    })

    it('throws OtpSecondChallengeError if retry also requires OTP', async () => {
      const context = createOtpMockContext()
      const operation = async () => {
        throw Object.assign(new Error('otp'), { code: 'EOTP' })
      }
      await expect(withOtpHandling({ context, fetchOptions, operation }))
        .rejects.toBeInstanceOf(OtpSecondChallengeError)
    })

    it('throws non-OTP errors from the retry as-is', async () => {
      let callCount = 0
      const retryError = new Error('server error')
      const context = createOtpMockContext()
      await expect(withOtpHandling({
        context,
        fetchOptions,
        operation: async () => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), { code: 'EOTP' })
          }
          throw retryError
        },
      })).rejects.toBe(retryError)
    })

    it('re-throws the original OTP error when enquirer returns no OTP', async () => {
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: '' }) },
      })
      await expect(withOtpHandling({
        context,
        fetchOptions,
        operation: async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
      })).rejects.toMatchObject({ code: 'EOTP' })
    })

    it('re-throws the original OTP error when enquirer returns undefined', async () => {
      const context = createOtpMockContext({
        enquirer: { prompt: async () => undefined },
      })
      await expect(withOtpHandling({
        context,
        fetchOptions,
        operation: async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
      })).rejects.toMatchObject({ code: 'EOTP' })
    })
  })

  describe('webauth flow', () => {
    it('polls doneUrl and uses returned token', async () => {
      let operationCallCount = 0
      let fetchCallCount = 0
      const globalInfo = jest.fn()
      const context = createOtpMockContext({
        globalInfo,
        fetch: async (): Promise<WebAuthFetchResponse> => {
          fetchCallCount++
          if (fetchCallCount < 3) {
            return createMockResponse({
              ok: true,
              status: 202,
              headers: { get: () => '1' },
            })
          }
          return createMockResponse({
            ok: true,
            status: 200,
            json: { token: 'web-token-123' },
          })
        },
      })
      const result = await withOtpHandling({
        context,
        fetchOptions,
        operation: async otp => {
          operationCallCount++
          if (operationCallCount === 1) {
            throw Object.assign(new Error('otp'), {
              code: 'EOTP',
              body: {
                authUrl: 'https://registry.npmjs.org/auth/abc',
                doneUrl: 'https://registry.npmjs.org/auth/abc/done',
              },
            })
          }
          expect(otp).toBe('web-token-123')
          return 'published'
        },
      })
      expect(result).toBe('published')
      expect(operationCallCount).toBe(2)
      expect(fetchCallCount).toBe(3)
      expect(globalInfo.mock.calls).toEqual([[expect.stringContaining('https://registry.npmjs.org/auth/abc')]])
    })

    it('falls back to classic prompt when only authUrl is present (no doneUrl)', async () => {
      let callCount = 0
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: 'manual-code' }) },
      })
      const result = await withOtpHandling({
        context,
        fetchOptions,
        operation: async otp => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), {
              code: 'EOTP',
              body: { authUrl: 'https://registry.npmjs.org/auth/abc' },
            })
          }
          expect(otp).toBe('manual-code')
          return 'done'
        },
      })
      expect(result).toBe('done')
    })

    it('falls back to classic prompt when only doneUrl is present (no authUrl)', async () => {
      let callCount = 0
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: 'manual-code' }) },
      })
      const result = await withOtpHandling({
        context,
        fetchOptions,
        operation: async otp => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), {
              code: 'EOTP',
              body: { doneUrl: 'https://registry.npmjs.org/auth/abc/done' },
            })
          }
          expect(otp).toBe('manual-code')
          return 'done'
        },
      })
      expect(result).toBe('done')
    })

    it('throws WebAuthTimeoutError when webauth polling times out', async () => {
      let time = 0
      const globalInfo = jest.fn()
      const context = createOtpMockContext({
        globalInfo,
        Date: { now: () => time },
        setTimeout: (cb: () => void) => {
          time += 6 * 60 * 1000
          cb()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => createMockResponse({
          ok: true,
          status: 202,
          headers: { get: () => null },
        }),
      })
      let called = false
      await expect(withOtpHandling({
        context,
        fetchOptions,
        operation: async () => {
          if (!called) {
            called = true
            throw Object.assign(new Error('otp'), {
              code: 'EOTP',
              body: {
                authUrl: 'https://registry.npmjs.org/auth/abc',
                doneUrl: 'https://registry.npmjs.org/auth/abc/done',
              },
            })
          }
          throw new Error('Unexpected second call to operation')
        },
      })).rejects.toBeInstanceOf(WebAuthTimeoutError)
      expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://registry.npmjs.org/auth/abc'))
    })
  })
})

describe('SyntheticOtpError', () => {
  it('has EOTP code', () => {
    const err = new SyntheticOtpError({ authUrl: 'https://example.com/auth', doneUrl: 'https://example.com/done' })
    expect(err.code).toBe('EOTP')
  })

  it('stores body', () => {
    const body = { authUrl: 'https://example.com/auth', doneUrl: 'https://example.com/done' }
    const err = new SyntheticOtpError(body)
    expect(err.body).toEqual(body)
  })
})

describe('SyntheticOtpError.fromUnknownBody', () => {
  const unexpectedWarn = (msg: string) => {
    throw new Error(`Unexpected call to globalWarn: ${msg}`)
  }

  it('extracts valid string authUrl and doneUrl', () => {
    const err = SyntheticOtpError.fromUnknownBody(unexpectedWarn, {
      authUrl: 'https://example.com/auth',
      doneUrl: 'https://example.com/done',
    })
    expect(err.body).toEqual({
      authUrl: 'https://example.com/auth',
      doneUrl: 'https://example.com/done',
    })
  })

  it('returns undefined body when body is null', () => {
    const err = SyntheticOtpError.fromUnknownBody(unexpectedWarn, null)
    expect(err.body).toBeUndefined()
  })

  it('returns undefined body when body is not an object', () => {
    const err = SyntheticOtpError.fromUnknownBody(unexpectedWarn, 'not an object')
    expect(err.body).toBeUndefined()
  })

  it('warns when authUrl has wrong type', () => {
    const globalWarn = jest.fn()
    const err = SyntheticOtpError.fromUnknownBody(globalWarn, {
      authUrl: 123,
      doneUrl: 'https://example.com/done',
    })
    expect(globalWarn.mock.calls).toEqual([[expect.stringContaining('authUrl')]])
    expect(err.body?.authUrl).toBeUndefined()
    expect(err.body?.doneUrl).toBe('https://example.com/done')
  })

  it('warns when doneUrl has wrong type', () => {
    const globalWarn = jest.fn()
    const err = SyntheticOtpError.fromUnknownBody(globalWarn, {
      authUrl: 'https://example.com/auth',
      doneUrl: true,
    })
    expect(globalWarn.mock.calls).toEqual([[expect.stringContaining('doneUrl')]])
    expect(err.body?.authUrl).toBe('https://example.com/auth')
    expect(err.body?.doneUrl).toBeUndefined()
  })

  it('warns for both when both have wrong types', () => {
    const globalWarn = jest.fn()
    const err = SyntheticOtpError.fromUnknownBody(globalWarn, {
      authUrl: 42,
      doneUrl: false,
    })
    expect(globalWarn.mock.calls).toEqual([
      [expect.stringContaining('authUrl')],
      [expect.stringContaining('doneUrl')],
    ])
    expect(err.body?.authUrl).toBeUndefined()
    expect(err.body?.doneUrl).toBeUndefined()
  })

  it('returns empty body when body has no authUrl or doneUrl', () => {
    const err = SyntheticOtpError.fromUnknownBody(unexpectedWarn, { something: 'else' })
    expect(err.body).toEqual({})
  })
})
