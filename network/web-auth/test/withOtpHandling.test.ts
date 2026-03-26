import {
  type OtpHandlingContext,
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  type WebAuthFetchOptions,
  type WebAuthFetchResponse,
  WebAuthTimeoutError,
  withOtpHandling,
} from '@pnpm/network.web-auth'

function createOtpMockContext (overrides?: Partial<OtpHandlingContext>): OtpHandlingContext {
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
    ...overrides,
  }
}

const fetchOptions: WebAuthFetchOptions = { method: 'GET' as const }

describe('withOtpHandling', () => {
  it('returns the result when the operation succeeds without OTP', async () => {
    const context = createOtpMockContext()
    const result = await withOtpHandling(async () => 'success', context, fetchOptions)
    expect(result).toBe('success')
  })

  it('throws non-OTP errors as-is', async () => {
    const error = new Error('network error')
    const context = createOtpMockContext()
    await expect(withOtpHandling(async () => {
      throw error
    }, context, fetchOptions))
      .rejects.toBe(error)
  })

  it('throws OtpNonInteractiveError when terminal is not interactive', async () => {
    const context = createOtpMockContext({
      process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
    })
    const operation = async () => {
      throw Object.assign(new Error('otp'), { code: 'EOTP' })
    }
    await expect(withOtpHandling(operation, context, fetchOptions))
      .rejects.toBeInstanceOf(OtpNonInteractiveError)
  })

  it('throws OtpNonInteractiveError when stdout is not interactive', async () => {
    const context = createOtpMockContext({
      process: { stdin: { isTTY: true }, stdout: { isTTY: false } },
    })
    const operation = async () => {
      throw Object.assign(new Error('otp'), { code: 'EOTP' })
    }
    await expect(withOtpHandling(operation, context, fetchOptions))
      .rejects.toBeInstanceOf(OtpNonInteractiveError)
  })

  describe('classic OTP flow', () => {
    it('prompts for OTP and retries operation', async () => {
      let callCount = 0
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: '654321' }) },
      })
      const result = await withOtpHandling(
        async (otp) => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), { code: 'EOTP' })
          }
          expect(otp).toBe('654321')
          return 'ok'
        },
        context,
        fetchOptions
      )
      expect(result).toBe('ok')
      expect(callCount).toBe(2)
    })

    it('throws OtpSecondChallengeError if retry also requires OTP', async () => {
      const context = createOtpMockContext()
      const operation = async () => {
        throw Object.assign(new Error('otp'), { code: 'EOTP' })
      }
      await expect(withOtpHandling(operation, context, fetchOptions))
        .rejects.toBeInstanceOf(OtpSecondChallengeError)
    })

    it('throws non-OTP errors from the retry as-is', async () => {
      let callCount = 0
      const retryError = new Error('server error')
      const context = createOtpMockContext()
      await expect(withOtpHandling(
        async () => {
          callCount++
          if (callCount === 1) {
            throw Object.assign(new Error('otp'), { code: 'EOTP' })
          }
          throw retryError
        },
        context,
        fetchOptions
      )).rejects.toBe(retryError)
    })

    it('re-throws the original OTP error when enquirer returns no OTP', async () => {
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: '' }) },
      })
      await expect(withOtpHandling(
        async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
        context,
        fetchOptions
      )).rejects.toMatchObject({ code: 'EOTP' })
    })

    it('re-throws the original OTP error when enquirer returns undefined', async () => {
      const context = createOtpMockContext({
        enquirer: { prompt: async () => undefined },
      })
      await expect(withOtpHandling(
        async () => {
          throw Object.assign(new Error('otp'), { code: 'EOTP' })
        },
        context,
        fetchOptions
      )).rejects.toMatchObject({ code: 'EOTP' })
    })
  })

  describe('webauth flow', () => {
    it('polls doneUrl and uses returned token', async () => {
      let operationCallCount = 0
      let fetchCallCount = 0
      const infoMessages: string[] = []
      const context = createOtpMockContext({
        globalInfo: (msg) => {
          infoMessages.push(msg)
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
      const result = await withOtpHandling(
        async (otp) => {
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
        context,
        fetchOptions
      )
      expect(result).toBe('published')
      expect(operationCallCount).toBe(2)
      expect(fetchCallCount).toBe(3)
      expect(infoMessages[0]).toContain('https://registry.npmjs.org/auth/abc')
    })

    it('falls back to classic prompt when only authUrl is present (no doneUrl)', async () => {
      let callCount = 0
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: 'manual-code' }) },
      })
      const result = await withOtpHandling(
        async (otp) => {
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
        context,
        fetchOptions
      )
      expect(result).toBe('done')
    })

    it('falls back to classic prompt when only doneUrl is present (no authUrl)', async () => {
      let callCount = 0
      const context = createOtpMockContext({
        enquirer: { prompt: async () => ({ otp: 'manual-code' }) },
      })
      const result = await withOtpHandling(
        async (otp) => {
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
        context,
        fetchOptions
      )
      expect(result).toBe('done')
    })

    it('throws WebAuthTimeoutError when webauth polling times out', async () => {
      let time = 0
      const context = createOtpMockContext({
        Date: { now: () => time },
        setTimeout: (cb: () => void) => {
          time += 6 * 60 * 1000
          cb()
        },
        fetch: async (): Promise<WebAuthFetchResponse> => ({
          headers: { get: () => null },
          json: async () => ({}),
          ok: true,
          status: 202,
        }),
      })
      let called = false
      await expect(withOtpHandling(
        async () => {
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
          return 'never'
        },
        context,
        fetchOptions
      )).rejects.toBeInstanceOf(WebAuthTimeoutError)
    })
  })
})
