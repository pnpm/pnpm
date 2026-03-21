import {
  generateQrCode,
  pollForWebAuthToken,
  type WebAuthContext,
  type WebAuthFetchResponse,
  WebAuthTimeoutError,
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

describe('generateQrCode', () => {
  it('returns a non-empty string', () => {
    const qr = generateQrCode('https://example.com')
    expect(typeof qr).toBe('string')
    expect(qr.length).toBeGreaterThan(0)
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
})
