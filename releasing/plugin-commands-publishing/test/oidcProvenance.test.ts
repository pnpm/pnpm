import { jest } from '@jest/globals'
import {
  type ProvenanceContext,
  type ProvenanceFetchOptions,
  ProvenanceMalformedIdTokenError,
  ProvenanceInsufficientInformationError,
  ProvenanceFailedToFetchVisibilityError,
  determineProvenance,
} from '../src/oidc/provenance.js'

describe('determineProvenance', () => {
  const registry = 'https://registry.npmjs.org'
  const packageName = '@pnpm/test-package'
  const authToken = 'test-auth-token'

  function createIdToken (payload: Record<string, unknown>): string {
    const header = { alg: 'RS256', typ: 'JWT' }
    const headerB64 = Buffer.from(JSON.stringify(header)).toString('base64url')
    const payloadB64 = Buffer.from(JSON.stringify(payload)).toString('base64url')
    return `${headerB64}.${payloadB64}.signature`
  }

  test('throws ProvenanceMalformedIdTokenError when idToken is malformed (no dots)', async () => {
    const mockFetch = jest.fn() as ProvenanceContext['fetch']
    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    await expect(determineProvenance({
      authToken,
      idToken: 'not-a-jwt-token',
      packageName,
      registry,
      context,
    })).rejects.toThrow(ProvenanceMalformedIdTokenError)

    expect(mockFetch).not.toHaveBeenCalled()
  })

  test('throws ProvenanceMalformedIdTokenError when idToken has only one part', async () => {
    const mockFetch = jest.fn() as ProvenanceContext['fetch']
    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    await expect(determineProvenance({
      authToken,
      idToken: 'header.',
      packageName,
      registry,
      context,
    })).rejects.toThrow(ProvenanceMalformedIdTokenError)
  })

  test('throws ProvenanceInsufficientInformationError for GitHub Actions with non-public repository', async () => {
    const mockFetch = jest.fn() as ProvenanceContext['fetch']
    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'private' })

    await expect(determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })).rejects.toThrow(ProvenanceInsufficientInformationError)

    expect(mockFetch).not.toHaveBeenCalled()
  })

  test('throws ProvenanceInsufficientInformationError for GitLab with non-public project', async () => {
    const mockFetch = jest.fn() as ProvenanceContext['fetch']
    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: false, GITLAB: true },
      fetch: mockFetch,
      process: { env: { SIGSTORE_ID_TOKEN: 'token' } },
    }

    const idToken = createIdToken({ project_visibility: 'private' })

    await expect(determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })).rejects.toThrow(ProvenanceInsufficientInformationError)

    expect(mockFetch).not.toHaveBeenCalled()
  })

  test('throws ProvenanceInsufficientInformationError for GitLab without SIGSTORE_ID_TOKEN', async () => {
    const mockFetch = jest.fn() as ProvenanceContext['fetch']
    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: false, GITLAB: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ project_visibility: 'public' })

    await expect(determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })).rejects.toThrow(ProvenanceInsufficientInformationError)

    expect(mockFetch).not.toHaveBeenCalled()
  })

  test('returns true when package is public in GitHub Actions', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ public: true }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const result = await determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    expect(result).toBe(true)
    expect(mockFetch).toHaveBeenCalledTimes(1)

    const expectedOptions = expect.objectContaining({
      headers: {
        Accept: 'application/json',
        Authorization: `Bearer ${authToken}`,
      },
      method: 'GET',
    } as Partial<ProvenanceFetchOptions>)
    expect(mockFetch).toHaveBeenCalledWith(
      expect.any(URL),
      expectedOptions
    )
    expect(mockFetch).toHaveBeenCalledWith(
      expect.objectContaining({
        href: expect.stringContaining(`/-/package/${encodeURIComponent(packageName)}/visibility`),
      }),
      expectedOptions
    )
  })

  test('returns true when package is public in GitLab', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ public: true }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: false, GITLAB: true },
      fetch: mockFetch,
      process: { env: { SIGSTORE_ID_TOKEN: 'token' } },
    }

    const idToken = createIdToken({ project_visibility: 'public' })

    const result = await determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    expect(result).toBe(true)
    expect(mockFetch).toHaveBeenCalledTimes(1)
  })

  test('returns undefined when package visibility is not public', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ public: false }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const result = await determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    expect(result).toBeUndefined()
  })

  test('returns undefined when visibility response is missing public field', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({}),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const result = await determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    expect(result).toBeUndefined()
  })

  test('passes fetch options correctly', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ public: true }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const options = {
      fetchRetries: 4,
      fetchRetryFactor: 2.5,
      fetchRetryMaxtimeout: 90000,
      fetchRetryMintimeout: 1500,
      fetchTimeout: 40000,
    }

    await determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
      options,
    })

    expect(mockFetch).toHaveBeenCalledTimes(1)
    expect(mockFetch).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        retry: {
          factor: options.fetchRetryFactor,
          maxTimeout: options.fetchRetryMaxtimeout,
          minTimeout: options.fetchRetryMintimeout,
          retries: options.fetchRetries,
        },
        timeout: options.fetchTimeout,
      } as Partial<ProvenanceFetchOptions>)
    )
  })

  test('throws ProvenanceFailedToFetchVisibilityError when fetch fails', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 404,
      json: async () => ({ code: 'NOT_FOUND', message: 'Package not found' }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    await expect(determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })).rejects.toThrow(ProvenanceFailedToFetchVisibilityError)

    const promise = determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    await expect(promise).rejects.toBeInstanceOf(ProvenanceFailedToFetchVisibilityError)
    await expect(promise).rejects.toHaveProperty(['status'], 404)
    await expect(promise).rejects.toHaveProperty(['packageName'], packageName)
    await expect(promise).rejects.toHaveProperty(['registry'], registry)
    await expect(promise).rejects.toHaveProperty(['errorResponse', 'code'], 'NOT_FOUND')
    await expect(promise).rejects.toHaveProperty(['errorResponse', 'message'], 'Package not found')
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed to fetch visibility for package @pnpm/test-package from registry https://registry.npmjs.org due to NOT_FOUND: Package not found (status code 404)'
    )
  })

  test('handles visibility fetch error with only code', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 401,
      json: async () => ({ code: 'UNAUTHORIZED' }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const promise = determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    await expect(promise).rejects.toBeInstanceOf(ProvenanceFailedToFetchVisibilityError)
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed to fetch visibility for package @pnpm/test-package from registry https://registry.npmjs.org due to UNAUTHORIZED (status code 401)'
    )
  })

  test('handles visibility fetch error with only message', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 500,
      json: async () => ({ message: 'Internal server error' }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const promise = determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    await expect(promise).rejects.toBeInstanceOf(ProvenanceFailedToFetchVisibilityError)
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed to fetch visibility for package @pnpm/test-package from registry https://registry.npmjs.org due to Internal server error (status code 500)'
    )
  })

  test('handles visibility fetch error with no error details', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 503,
      json: async () => ({}),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const promise = determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    await expect(promise).rejects.toBeInstanceOf(ProvenanceFailedToFetchVisibilityError)
    await expect(promise).rejects.toHaveProperty(
      ['message'],
      'Failed to fetch visibility for package @pnpm/test-package from registry https://registry.npmjs.org due to an unknown error (status code 503)'
    )
  })

  test('handles visibility fetch error when JSON parsing fails', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: false,
      status: 500,
      json: async () => {
        throw new Error('JSON parse error')
      },
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })

    const promise = determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    await expect(promise).rejects.toBeInstanceOf(ProvenanceFailedToFetchVisibilityError)
    await expect(promise).rejects.toHaveProperty(['status'], 500)
    await expect(promise).rejects.toHaveProperty(['errorResponse'], undefined)
  })

  test('encodes package name in URL', async () => {
    const mockFetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ public: true }),
    }))

    const context: ProvenanceContext = {
      ciInfo: { GITHUB_ACTIONS: true },
      fetch: mockFetch,
      process: { env: {} },
    }

    const idToken = createIdToken({ repository_visibility: 'public' })
    const packageName = '@scope/package'

    await determineProvenance({
      authToken,
      idToken,
      packageName,
      registry,
      context,
    })

    expect(mockFetch.mock.calls).toStrictEqual([[
      expect.any(URL),
      expect.anything(),
    ]])

    expect(mockFetch.mock.calls).toStrictEqual([[
      expect.objectContaining({
        href: expect.stringContaining(encodeURIComponent(packageName)),
      }),
      expect.anything(),
    ]])
  })
})
