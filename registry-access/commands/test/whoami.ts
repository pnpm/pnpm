import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { whoami } from '../src/index.js'

const REGISTRY = 'https://registry.npmjs.org'
const REGISTRY_URL = `${REGISTRY}/`
const AUTH_HEADER = 'Bearer test-token'
const CONFIG_BY_URI = {
  '//registry.npmjs.org/': {
    creds: {
      authToken: 'test-token',
    },
  },
}

describe('whoami', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('returns the current username', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'GET',
      path: '/-/whoami',
      headers: { authorization: AUTH_HEADER },
    }).reply(200, JSON.stringify({ username: 'alice' }))

    const result = await whoami.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    })

    expect(result).toBe('alice')
  })

  it('throws when not logged in', async () => {
    await expect(whoami.handler({
      registries: { default: REGISTRY_URL },
    })).rejects.toThrow('You must be logged in')
  })

  it('throws when the registry rejects the request', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'GET',
      path: '/-/whoami',
    }).reply(401, '{}')

    await expect(whoami.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    })).rejects.toThrow('Failed to find the current user')
  })

  it('preserves a registry path prefix when the URL has no trailing slash', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'GET',
      path: '/custom-prefix/-/whoami',
    }).reply(200, JSON.stringify({ username: 'alice' }))

    const result = await whoami.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: `${REGISTRY}/custom-prefix` },
    })

    expect(result).toBe('alice')
  })
})
