import {
  generateQrCode,
  type OtpHandlingContext,
  OtpNonInteractiveError,
  OtpSecondChallengeError,
  pollForWebAuthToken,
  type WebAuthContext,
  type WebAuthFetchOptions,
  type WebAuthFetchResponse,
  WebAuthTimeoutError,
  withOtpHandling,
} from '@pnpm/network.web-auth'

function createMockContext (overrides?: Partial<WebAuthContext>): WebAuthContext {
  return {
    Date: { now: () => 0 },
    setTimeout: (cb: () => void) => cb(),
    fetch: async () => ({
      headers: { get: () => null },
      json: async () => ({}),
      ok: false,
      status: 404,
    }),
    ...overrides,
  }
}

const fetchOptions = { method: 'GET' as const }

describe('WebAuthTimeoutError', () => {
  it('stores endTime, startTime, and timeout', () => {
    const err = new WebAuthTimeoutError(310_000, 10_000, 300_000)
    expect(err.endTime).toBe(310_000)
    expect(err.startTime).toBe(10_000)
    expect(err.timeout).toBe(300_000)
  })

  it('has ERR_PNPM_WEBAUTH_TIMEOUT code', () => {
    const err = new WebAuthTimeoutError(0, 0, 0)
    expect(err.code).toBe('ERR_PNPM_WEBAUTH_TIMEOUT')
  })

  it('includes a hint about re-running the command', () => {
    const err = new WebAuthTimeoutError(0, 0, 0)
    expect(err.hint).toMatch(/Re-run/)
  })

  it('has a descriptive message', () => {
    const err = new WebAuthTimeoutError(0, 0, 0)
    expect(err.message).toMatch(/timed out/)
  })
})

describe('generateQrCode', () => {
  it('returns a non-empty string', () => {
    const qr = generateQrCode('https://example.com')
    expect(typeof qr).toBe('string')
    expect(qr.length).toBeGreaterThan(0)
  })

  it('produces different output for different inputs', () => {
    const qr1 = generateQrCode('https://example.com/a')
    const qr2 = generateQrCode('https://example.com/b')
    expect(qr1).not.toBe(qr2)
  })
})

describe('pollForWebAuthToken', () => {
  it('returns token when doneUrl responds with 200 and token', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
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
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('web-token-123')
    expect(fetchCallCount).toBe(3)
  })

  it('passes doneUrl and fetchOptions to fetch', async () => {
    const capturedArgs: Array<{ url: string, options: WebAuthFetchOptions }> = []
    const opts: WebAuthFetchOptions = {
      method: 'GET',
      timeout: 5000,
      retry: { retries: 3 },
    }
    const context = createMockContext({
      fetch: async (url: string, options: WebAuthFetchOptions): Promise<WebAuthFetchResponse> => {
        capturedArgs.push({ url, options })
        return {
          headers: { get: () => null },
          json: async () => ({ token: 'tok' }),
          ok: true,
          status: 200,
        }
      },
    })
    await pollForWebAuthToken('https://registry.example.com/done', context, opts)
    expect(capturedArgs).toHaveLength(1)
    expect(capturedArgs[0].url).toBe('https://registry.example.com/done')
    expect(capturedArgs[0].options).toBe(opts)
  })

  it('respects Retry-After header when polling', async () => {
    const setTimeoutDelays: number[] = []
    let fetchCallCount = 0
    const context = createMockContext({
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
    await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    // First setTimeout is the default 1s poll interval,
    // second is the additional delay (5s Retry-After minus the 1s already waited),
    // third is the default 1s poll interval for the next iteration.
    expect(setTimeoutDelays).toStrictEqual([1000, 4000, 1000])
  })

  it('ignores Retry-After when value is not a finite number', async () => {
    const setTimeoutDelays: number[] = []
    let fetchCallCount = 0
    const context = createMockContext({
      setTimeout: (cb: () => void, ms: number) => {
        setTimeoutDelays.push(ms)
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return {
            headers: { get: () => 'not-a-number' },
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
    await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    // Only the default 1s poll intervals, no additional Retry-After delay.
    expect(setTimeoutDelays).toStrictEqual([1000, 1000])
  })

  it('ignores Retry-After when value is null', async () => {
    const setTimeoutDelays: number[] = []
    let fetchCallCount = 0
    const context = createMockContext({
      setTimeout: (cb: () => void, ms: number) => {
        setTimeoutDelays.push(ms)
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return {
            headers: { get: () => null },
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
    await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(setTimeoutDelays).toStrictEqual([1000, 1000])
  })

  it('skips additional delay when Retry-After is less than poll interval', async () => {
    const setTimeoutDelays: number[] = []
    let fetchCallCount = 0
    const context = createMockContext({
      setTimeout: (cb: () => void, ms: number) => {
        setTimeoutDelays.push(ms)
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return {
            headers: { get: (name: string) => name === 'retry-after' ? '0.5' : null },
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
    await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    // Retry-After of 0.5s (500ms) is less than the 1s poll interval already waited,
    // so no additional delay is added.
    expect(setTimeoutDelays).toStrictEqual([1000, 1000])
  })

  it('caps Retry-After additional delay to remaining timeout', async () => {
    let time = 0
    const setTimeoutDelays: number[] = []
    const context = createMockContext({
      Date: { now: () => time },
      setTimeout: (cb: () => void, ms: number) => {
        setTimeoutDelays.push(ms)
        time += ms
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => ({
        headers: { get: (name: string) => name === 'retry-after' ? '60' : null },
        json: async () => ({ token: 'tok' }),
        ok: true,
        status: 202,
      }),
    })
    // Use a 10s timeout so the 60s Retry-After gets capped.
    await expect(pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions, 10_000))
      .rejects.toBeInstanceOf(WebAuthTimeoutError)
    // The first delay is the 1s poll interval. The additional delay from
    // Retry-After (59s) should be capped to the remaining timeout (~9s).
    expect(setTimeoutDelays[0]).toBe(1000)
    expect(setTimeoutDelays[1]).toBeLessThanOrEqual(9000)
  })

  it('throws WebAuthTimeoutError when timeout expires during Retry-After wait', async () => {
    let time = 0
    const timeoutMs = 5000
    const context = createMockContext({
      Date: {
        now: () => time,
      },
      setTimeout: (cb: () => void, ms: number) => {
        time += ms
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => {
        // After the 1s poll interval, time is 1000.
        // Remaining is 4000. Retry-After is 100s, so additional is 99000,
        // capped to 4000. After that wait, time = 5000, which equals timeout.
        // Next iteration: now - startTime > timeoutMs → throw.
        return {
          headers: { get: (name: string) => name === 'retry-after' ? '100' : null },
          json: async () => ({}),
          ok: true,
          status: 202,
        }
      },
    })
    await expect(pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions, timeoutMs))
      .rejects.toMatchObject({ timeout: timeoutMs })
  })

  it('continues polling when fetch throws', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
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
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when response is not ok', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
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
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when response.json() throws', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
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
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when response body has no token', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return {
            headers: { get: () => null },
            json: async () => ({ something: 'else' }),
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
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when token is empty string', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return {
            headers: { get: () => null },
            json: async () => ({ token: '' }),
            ok: true,
            status: 200,
          }
        }
        return {
          headers: { get: () => null },
          json: async () => ({ token: 'real-tok' }),
          ok: true,
          status: 200,
        }
      },
    })
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('real-tok')
    expect(fetchCallCount).toBe(2)
  })

  it('throws WebAuthTimeoutError after timeout', async () => {
    let time = 0
    const context = createMockContext({
      Date: { now: () => time },
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
    await expect(pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions))
      .rejects.toBeInstanceOf(WebAuthTimeoutError)
  })

  it('uses custom timeout value', async () => {
    let time = 0
    const customTimeoutMs = 3000
    const context = createMockContext({
      Date: { now: () => time },
      setTimeout: (cb: () => void) => {
        time += 2000
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => ({
        headers: { get: () => null },
        json: async () => ({}),
        ok: true,
        status: 202,
      }),
    })
    await expect(pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions, customTimeoutMs))
      .rejects.toMatchObject({ timeout: customTimeoutMs })
  })

  it('recovers after multiple consecutive fetch errors', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount <= 5) {
          throw new Error(`failure #${fetchCallCount}`)
        }
        return {
          headers: { get: () => null },
          json: async () => ({ token: 'recovered' }),
          ok: true,
          status: 200,
        }
      },
    })
    const token = await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    expect(token).toBe('recovered')
    expect(fetchCallCount).toBe(6)
  })

  it('waits pollIntervalMs before each fetch call', async () => {
    const setTimeoutDelays: number[] = []
    let fetchCallCount = 0
    const context = createMockContext({
      setTimeout: (cb: () => void, ms: number) => {
        setTimeoutDelays.push(ms)
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount < 4) {
          return {
            headers: { get: () => null },
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
    await pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions)
    // Each iteration waits 1000ms before fetching.
    expect(setTimeoutDelays).toStrictEqual([1000, 1000, 1000, 1000])
  })

  it('throws WebAuthTimeoutError immediately when remaining time is zero during Retry-After', async () => {
    let time = 0
    const timeoutMs = 2000
    let fetchCallCount = 0
    const context = createMockContext({
      Date: { now: () => time },
      setTimeout: (cb: () => void, ms: number) => {
        time += ms
        cb()
      },
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          // After poll interval (1s), time = 1000, remaining = 1000.
          // Retry-After = 10s → additional = 9000 > remaining.
          // Capped to remaining (1000). After that wait, time = 2000.
          return {
            headers: { get: (name: string) => name === 'retry-after' ? '10' : null },
            json: async () => ({}),
            ok: true,
            status: 202,
          }
        }
        // This second fetch still returns 202, but the next timeout check
        // should trigger the error since time (2000) - start (0) = 2000 > 2000? No, it's equal.
        // Actually the condition is `>` so 2000 > 2000 is false. So it waits another 1s, then 3000 > 2000 is true.
        return {
          headers: { get: () => null },
          json: async () => ({}),
          ok: true,
          status: 202,
        }
      },
    })
    await expect(pollForWebAuthToken('https://registry.npmjs.org/auth/done', context, fetchOptions, timeoutMs))
      .rejects.toMatchObject({ timeout: timeoutMs })
  })
})

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
