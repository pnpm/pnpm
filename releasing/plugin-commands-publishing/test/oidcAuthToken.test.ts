import { jest } from '@jest/globals'
import {
  type AuthTokenContext,
  type AuthTokenFetchOptions,
  AuthTokenFetchError,
  AuthTokenExchangeError,
  AuthTokenJsonInterruptedError,
  AuthTokenMalformedJsonError,
  fetchAuthToken,
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

    expect(mockFetch).toHaveBeenCalledWith(
      'https://registry.npmjs.org/-/npm/v1/oidc/token/exchange/package/%40pnpm%2Ftest-package',
      expect.objectContaining({
        headers: {
          Accept: 'application/json',
          Authorization: `Bearer ${idToken}`,
          'Content-Length': '0',
        },
        body: '',
        method: 'POST',
      } as AuthTokenFetchOptions)
    )
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

    const packageName = '@scope/package'
    await fetchAuthToken({ context, idToken, packageName, registry })

    expect(mockFetch).toHaveBeenCalledWith(
      `${registry}/-/npm/v1/oidc/token/exchange/package/${encodeURIComponent(packageName)}`,
      expect.anything()
    )
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

    expect(mockFetch).toHaveBeenCalledWith(
      expect.any(String),
      {
        body: '',
        headers: {
          Accept: 'application/json',
          Authorization: `Bearer ${idToken}`,
          'Content-Length': '0',
        },
        method: 'POST',
        retry: {
          factor: 3,
          maxTimeout: 120000,
          minTimeout: 2000,
          retries: 5,
        },
        timeout: 45000,
      }
    )
  })

  test('throws AuthTokenFetchError when fetch fails', async () => {
    const fetchError = new Error('Network error')
    const mockFetch = jest.fn(async () => {
      throw fetchError
    })

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenFetchError)
    await expect(promise).rejects.toHaveProperty(['errorSource'], fetchError)
    await expect(promise).rejects.toHaveProperty(['packageName'], packageName)
    await expect(promise).rejects.toHaveProperty(['registry'], registry)
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_AUTH_TOKEN_FETCH')
  })

  test('throws AuthTokenExchangeError when response is not ok and returns a payload of error', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 401,
      json: async () => ({ body: { message: 'Unauthorized' } }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenExchangeError)
    await expect(promise).rejects.toHaveProperty(['httpStatus'], 401)
    await expect(promise).rejects.toHaveProperty(['errorResponse', 'body', 'message'], 'Unauthorized')
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_AUTH_TOKEN_EXCHANGE')
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed token exchange request with body message: Unauthorized (status code 401)'
    )
  })

  test('throws AuthTokenExchangeError when response is not ok and the returned payload could not be fetched', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 401,
      json: async () => {
        throw new Error('no json')
      },
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenExchangeError)
    await expect(promise).rejects.toHaveProperty(['httpStatus'], 401)
    await expect(promise).rejects.toHaveProperty(['errorResponse'], undefined)
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_AUTH_TOKEN_EXCHANGE')
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed token exchange request with body message: Unknown error (status code 401)'
    )
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

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenExchangeError)
    await expect(promise).rejects.toHaveProperty(['httpStatus'], 403)
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed token exchange request with body message: Unknown error (status code 403)'
    )
  })

  test('handles exchange error when json response is valid', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 500,
      json: async () => ({ body: { message: 'Internal Server Error' } }),
    }))

    const context: AuthTokenContext = {
      fetch: mockFetch,
    }

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenExchangeError)
    await expect(promise).rejects.toHaveProperty(['httpStatus'], 500)
    await expect(promise).rejects.toHaveProperty(['errorResponse', 'body', 'message'], 'Internal Server Error')
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

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenJsonInterruptedError)
    await expect(promise).rejects.toHaveProperty(['errorSource'], jsonError)
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_AUTH_TOKEN_JSON_INTERRUPTED')
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

    const promise = fetchAuthToken({ context, idToken, packageName, registry })

    await expect(promise).rejects.toBeInstanceOf(AuthTokenMalformedJsonError)
    await expect(promise).rejects.toHaveProperty(['malformedJsonResponse'], {})
    await expect(promise).rejects.toHaveProperty(['packageName'], packageName)
    await expect(promise).rejects.toHaveProperty(['registry'], registry)
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_AUTH_TOKEN_MALFORMED_JSON')
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

    await expect(fetchAuthToken({ context, idToken, packageName, registry })).rejects.toThrow(AuthTokenMalformedJsonError)
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

    await expect(fetchAuthToken({ context, idToken, packageName, registry })).rejects.toThrow(AuthTokenMalformedJsonError)
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

    await expect(fetchAuthToken({ context, idToken, packageName, registry })).rejects.toThrow(AuthTokenMalformedJsonError)
  })
})
