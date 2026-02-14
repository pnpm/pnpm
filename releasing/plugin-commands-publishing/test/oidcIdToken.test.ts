import { jest } from '@jest/globals'
import {
  type IdTokenContext,
  type IdTokenFetchOptions,
  IdTokenGitHubWorkflowIncorrectPermissionsError,
  IdTokenGitHubInvalidResponseError,
  IdTokenGitHubJsonInterruptedError,
  IdTokenGitHubJsonInvalidValueError,
  getIdToken,
} from '../src/oidc/idToken.js'

describe('getIdToken', () => {
  const registry = 'https://registry.npmjs.org'

  test('returns undefined when not in GitHub Actions or GitLab', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: false, GITLAB: false },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn() as IdTokenContext['globalInfo'],
      process: { env: {} },
    }

    const result = await getIdToken({ context, registry })

    expect(result).toBeUndefined()
    expect(context.fetch).not.toHaveBeenCalled()
  })

  test('returns NPM_ID_TOKEN from environment when available', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn() as IdTokenContext['globalInfo'],
      process: { env: { NPM_ID_TOKEN: 'test-token-from-env' } },
    }

    const result = await getIdToken({ context, registry })

    expect(result).toBe('test-token-from-env')
    expect(context.fetch).not.toHaveBeenCalled()
  })

  test('returns NPM_ID_TOKEN from environment in GitLab', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: false, GITLAB: true },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn() as IdTokenContext['globalInfo'],
      process: { env: { NPM_ID_TOKEN: 'test-token-gitlab' } },
    }

    const result = await getIdToken({ context, registry })

    expect(result).toBe('test-token-gitlab')
    expect(context.fetch).not.toHaveBeenCalled()
  })

  test('returns undefined for GitLab when NPM_ID_TOKEN is not set', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: false, GITLAB: true },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn() as IdTokenContext['globalInfo'],
      process: { env: {} },
    }

    const result = await getIdToken({ context, registry })

    expect(result).toBeUndefined()
    expect(context.fetch).not.toHaveBeenCalled()
  })

  test('throws error when GitHub Actions environment variables are missing', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn(),
      process: { env: {} },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubWorkflowIncorrectPermissionsError)
  })

  test('throws error when only ACTIONS_ID_TOKEN_REQUEST_TOKEN is set', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn(),
      process: { env: { ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'token' } },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubWorkflowIncorrectPermissionsError)
  })

  test('throws error when only ACTIONS_ID_TOKEN_REQUEST_URL is set', async () => {
    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: jest.fn() as IdTokenContext['fetch'],
      globalInfo: jest.fn(),
      process: { env: { ACTIONS_ID_TOKEN_REQUEST_URL: 'https://example.com' } },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubWorkflowIncorrectPermissionsError)
  })

  test('fetches ID token from GitHub Actions successfully', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ value: 'fetched-id-token' }),
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    const result = await getIdToken({ context, registry })

    expect(result).toBe('fetched-id-token')
    expect(mockFetch).toHaveBeenCalledTimes(1)

    expect(mockFetch).toHaveBeenCalledWith(
      'https://actions.example.com/token?audience=npm%3Aregistry.npmjs.org',
      expect.objectContaining({
        headers: {
          Accept: 'application/json',
          Authorization: 'Bearer request-token',
        },
        method: 'GET',
      } as Partial<IdTokenFetchOptions>)
    )
  })

  test('passes fetch options correctly', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ value: 'token' }),
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    const options = {
      fetchRetries: 3,
      fetchRetryFactor: 2,
      fetchRetryMaxtimeout: 60000,
      fetchRetryMintimeout: 1000,
      fetchTimeout: 30000,
    }

    await getIdToken({ context, registry, options })

    expect(mockFetch).toHaveBeenCalledTimes(1)
    expect(mockFetch).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        retry: {
          factor: options.fetchRetryFactor,
          maxTimeout: options.fetchRetryMaxtimeout,
          minTimeout: options.fetchRetryMintimeout,
          retries: options.fetchRetries,
        },
        timeout: options.fetchTimeout,
      } as Partial<IdTokenFetchOptions>)
    )
  })

  test('logs fetch information via globalInfo', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ value: 'token' }),
    }))
    const mockGlobalInfo = jest.fn()

    let dateIndex = 0
    const mockDateNowTable = [1000, 1500]
    const mockDateNow = jest.fn(() => {
      const result = mockDateNowTable[dateIndex]
      dateIndex += 1
      return result
    })

    const context: IdTokenContext = {
      Date: { now: mockDateNow },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: mockGlobalInfo,
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    await getIdToken({ context, registry })

    expect(mockDateNow).toHaveBeenCalledTimes(2)
    expect(mockGlobalInfo).toHaveBeenCalledWith('GET https://actions.example.com/token?audience=npm%3Aregistry.npmjs.org 200 500ms')
  })

  test('throws error when fetch response is not ok', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 401,
      json: async () => ({ code: 'UNAUTHORIZED', message: 'Unauthorized' }),
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubInvalidResponseError)
  })

  test('throws error when JSON parsing fails', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => {
        throw new Error('JSON parse error')
      },
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubJsonInterruptedError)
  })

  test('throws error when JSON response is missing value field', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({}),
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubJsonInvalidValueError)
  })

  test('throws error when JSON response value is not a string', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ value: 123 }),
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubJsonInvalidValueError)
  })

  test('throws error when JSON response is null', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => null,
    }))

    const context: IdTokenContext = {
      Date: { now: jest.fn(() => 1000) },
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      globalInfo: jest.fn(),
      process: {
        env: {
          ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
          ACTIONS_ID_TOKEN_REQUEST_URL: 'https://actions.example.com/token',
        },
      },
    }

    await expect(getIdToken({ context, registry })).rejects.toThrow(IdTokenGitHubJsonInvalidValueError)
  })
})
