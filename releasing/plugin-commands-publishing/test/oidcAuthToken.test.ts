import { jest } from '@jest/globals'
import {
  fetchAuthToken,
  AuthTokenFetchError,
  AuthTokenExchangeError,
  AuthTokenJsonInterruptedError,
  AuthTokenMalformedJsonError,
  type AuthTokenContext,
} from '../src/oidc/authToken.js'

describe('fetchAuthToken', () => {
  const registry = 'https://registry.npmjs.org'
  const packageName = '@pnpm/test-package'
  const idToken = 'test-id-token'

  test('successfully fetches auth token', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ token: 'fetched-auth-token' }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const result = await fetchAuthToken({ context, idToken, packageName, registry })

    expect(result).toBe('fetched-auth-token')
    expect(mockFetch).toHaveBeenCalledTimes(1)

    const [url, options] = mockFetch.mock.calls[0]
    expect(url).toContain('/-/npm/v1/oidc/token/exchange/package/')
    expect(url).toContain(encodeURIComponent(packageName))
    expect(options.headers.Authorization).toBe(`Bearer ${idToken}`)
    expect(options.method).toBe('POST')
    expect(options.body).toBe('')
    expect(options.headers['Content-Length']).toBe('0')
  })

  test('encodes package name in URL', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ token: 'token' }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const specialPackageName = '@scope/package with spaces'
    await fetchAuthToken({ context, idToken, packageName: specialPackageName, registry })

    const [url] = mockFetch.mock.calls[0]
    expect(url).toContain(encodeURIComponent(specialPackageName))
    expect(url).not.toContain('package with spaces')
  })

  test('passes fetch options correctly', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ token: 'token' }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const options = {
      fetchRetries: 5,
      fetchRetryFactor: 3,
      fetchRetryMaxtimeout: 120000,
      fetchRetryMintimeout: 2000,
      fetchTimeout: 45000,
    }

    await fetchAuthToken({ context, idToken, packageName, registry, options })

    expect(mockFetch).toHaveBeenCalledTimes(1)
    const [, fetchOptions] = mockFetch.mock.calls[0]
    expect(fetchOptions.retry).toEqual({
      factor: 3,
      maxTimeout: 120000,
      minTimeout: 2000,
      retries: 5,
    })
    expect(fetchOptions.timeout).toBe(45000)
  })

  test('throws AuthTokenFetchError when fetch fails', async () => {
    const fetchError = new Error('Network error')
    const mockFetch = jest.fn(async () => {
      throw fetchError
    })

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenFetchError)

    try {
      await fetchAuthToken({ context, idToken, packageName, registry })
    } catch (error) {
      if (error instanceof AuthTokenFetchError) {
        expect(error.errorSource).toBe(fetchError)
        expect(error.packageName).toBe(packageName)
        expect(error.registry).toBe(registry)
        expect(error.code).toBe('ERR_PNPM_AUTH_TOKEN_FETCH')
      }
    }
  })

  test('throws AuthTokenExchangeError when response is not ok', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 401,
      json: async () => ({ body: { message: 'Unauthorized' } }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenExchangeError)

    try {
      await fetchAuthToken({ context, idToken, packageName, registry })
    } catch (error) {
      if (error instanceof AuthTokenExchangeError) {
        expect(error.httpStatus).toBe(401)
        expect(error.errorResponse?.body?.message).toBe('Unauthorized')
        expect(error.code).toBe('ERR_PNPM_AUTH_TOKEN_EXCHANGE')
      }
    }
  })

  test('handles exchange error with missing body message', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 403,
      json: async () => ({}),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    try {
      await fetchAuthToken({ context, idToken, packageName, registry })
    } catch (error) {
      if (error instanceof AuthTokenExchangeError) {
        expect(error.message).toContain('Unknown error')
        expect(error.httpStatus).toBe(403)
      }
    }
  })

  test('handles exchange error when json parsing fails', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 500,
      json: async () => {
        throw new Error('JSON parse error')
      },
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    try {
      await fetchAuthToken({ context, idToken, packageName, registry })
    } catch (error) {
      if (error instanceof AuthTokenExchangeError) {
        expect(error.httpStatus).toBe(500)
        expect(error.errorResponse).toBeUndefined()
      }
    }
  })

  test('throws AuthTokenJsonInterruptedError when JSON parsing fails on success response', async () => {
    const jsonError = new Error('JSON parse error')
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => {
        throw jsonError
      },
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenJsonInterruptedError)

    try {
      await fetchAuthToken({ context, idToken, packageName, registry })
    } catch (error) {
      if (error instanceof AuthTokenJsonInterruptedError) {
        expect(error.errorSource).toBe(jsonError)
        expect(error.code).toBe('ERR_PNPM_AUTH_TOKEN_JSON_INTERRUPTED')
      }
    }
  })

  test('throws AuthTokenMalformedJsonError when JSON response is missing token', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({}),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenMalformedJsonError)

    try {
      await fetchAuthToken({ context, idToken, packageName, registry })
    } catch (error) {
      if (error instanceof AuthTokenMalformedJsonError) {
        expect(error.malformedJsonResponse).toEqual({})
        expect(error.packageName).toBe(packageName)
        expect(error.registry).toBe(registry)
        expect(error.code).toBe('ERR_PNPM_AUTH_TOKEN_MALFORMED_JSON')
      }
    }
  })

  test('throws AuthTokenMalformedJsonError when token is not a string', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ token: 12345 }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenMalformedJsonError)
  })

  test('throws AuthTokenMalformedJsonError when JSON response is null', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => null,
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenMalformedJsonError)
  })

  test('throws AuthTokenMalformedJsonError when JSON response is not an object', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => 'string response',
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    await expect(fetchAuthToken({ context, idToken, packageName, registry }))
      .rejects.toThrow(AuthTokenMalformedJsonError)
  })
})
