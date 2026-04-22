import { describe, expect, it } from '@jest/globals'
import {
  pollForWebAuthToken,
  type WebAuthContext,
  type WebAuthFetchOptions,
  type WebAuthFetchResponse,
  WebAuthTimeoutError,
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

const createMockContext = (overrides?: Partial<WebAuthContext>): WebAuthContext => ({
  Date: { now: () => 0 },
  setTimeout: (cb: () => void) => cb(),
  fetch: async () => createMockResponse({
    ok: false,
    status: 404,
  }),
  ...overrides,
})

const fetchOptions: WebAuthFetchOptions = { method: 'GET' }

describe('pollForWebAuthToken', () => {
  it('returns token when doneUrl responds with 200 and token', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
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
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    await pollForWebAuthToken({ context, doneUrl: 'https://registry.example.com/done', fetchOptions: opts })
    expect(capturedArgs).toEqual([{ url: 'https://registry.example.com/done', options: opts }])
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
          return createMockResponse({
            ok: true,
            status: 202,
            headers: { get: (name: string) => name === 'retry-after' ? '5' : null },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
          return createMockResponse({
            ok: true,
            status: 202,
            headers: { get: () => 'not-a-number' },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
          return createMockResponse({
            ok: true,
            status: 202,
            headers: { get: () => null },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
          return createMockResponse({
            ok: true,
            status: 202,
            headers: { get: (name: string) => name === 'retry-after' ? '0.5' : null },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
      fetch: async (): Promise<WebAuthFetchResponse> => createMockResponse({
        ok: true,
        status: 202,
        json: { token: 'tok' },
        headers: { get: (name: string) => name === 'retry-after' ? '60' : null },
      }),
    })
    // Use a 10s timeout so the 60s Retry-After gets capped.
    await expect(pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions, timeoutMs: 10_000 }))
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
        return createMockResponse({
          ok: true,
          status: 202,
          headers: { get: (name: string) => name === 'retry-after' ? '100' : null },
        })
      },
    })
    await expect(pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions, timeoutMs }))
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
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when response is not ok', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return createMockResponse({
            ok: false,
            status: 404,
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when response body has no token', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return createMockResponse({
            ok: true,
            status: 200,
            json: { something: 'else' },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
    expect(token).toBe('tok')
    expect(fetchCallCount).toBe(2)
  })

  it('continues polling when token is empty string', async () => {
    let fetchCallCount = 0
    const context = createMockContext({
      fetch: async (): Promise<WebAuthFetchResponse> => {
        fetchCallCount++
        if (fetchCallCount === 1) {
          return createMockResponse({
            ok: true,
            status: 200,
            json: { token: '' },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'real-tok' },
        })
      },
    })
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
      fetch: async (): Promise<WebAuthFetchResponse> => createMockResponse({
        ok: true,
        status: 202,
        headers: { get: () => null },
      }),
    })
    await expect(pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions }))
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
      fetch: async (): Promise<WebAuthFetchResponse> => createMockResponse({
        ok: true,
        status: 202,
        headers: { get: () => null },
      }),
    })
    await expect(pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions, timeoutMs: customTimeoutMs }))
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
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'recovered' },
        })
      },
    })
    const token = await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
          return createMockResponse({
            ok: true,
            status: 202,
            headers: { get: () => null },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    await pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions })
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
          return createMockResponse({
            ok: true,
            status: 202,
            headers: { get: (name: string) => name === 'retry-after' ? '10' : null },
          })
        }
        // This second fetch still returns 202, but the next timeout check
        // should trigger the error since time (2000) - start (0) = 2000 > 2000? No, it's equal.
        // Actually the condition is `>` so 2000 > 2000 is false. So it waits another 1s, then 3000 > 2000 is true.
        return createMockResponse({
          ok: true,
          status: 202,
          headers: { get: () => null },
        })
      },
    })
    await expect(pollForWebAuthToken({ context, doneUrl: 'https://registry.npmjs.org/auth/done', fetchOptions, timeoutMs }))
      .rejects.toMatchObject({ timeout: timeoutMs })
  })
})
