import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { star, stars, unstar } from '../src/index.js'

const REGISTRY = 'https://registry.npmjs.org'
const REGISTRY_URL = `${REGISTRY}/`
const CONFIG_BY_URI = {
  '//registry.npmjs.org/': {
    creds: {
      authToken: 'test-token',
    },
  },
}

describe('star', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('stars a package via the v1 endpoint', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'PUT',
      path: '/-/user/v1/star',
    }).reply(200, '{}')

    await expect(star.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, ['foo'])).resolves.toBeUndefined()
  })

  it('falls back to the legacy endpoint when the modern endpoints fail', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'PUT',
      path: '/-/user/v1/star',
    }).reply(404, '{}')
    mockPool.intercept({
      method: 'PUT',
      path: '/-/user/package/foo/star',
    }).reply(404, '{}')
    mockPool.intercept({
      method: 'GET',
      path: '/-/whoami',
    }).reply(200, JSON.stringify({ username: 'alice' }))
    mockPool.intercept({
      method: 'GET',
      path: '/foo',
    }).reply(200, JSON.stringify({ name: 'foo', _rev: 'rev-1' }))
    mockPool.intercept({
      method: 'PUT',
      path: '/foo/-rev/rev-1',
    }).reply(200, '{}')

    await expect(star.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, ['foo'])).resolves.toBeUndefined()
  })

  it('throws when no package name is given', async () => {
    await expect(star.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, [])).rejects.toThrow('Package name is required')
  })

  it('throws when not logged in', async () => {
    await expect(star.handler({
      registries: { default: REGISTRY_URL },
    }, ['foo'])).rejects.toThrow('You must be logged in to star')
  })
})

describe('unstar', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('unstars a package via the v1 endpoint', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'DELETE',
      path: '/-/user/v1/star',
    }).reply(200, '{}')

    await expect(unstar.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, ['foo'])).resolves.toBeUndefined()
  })

  it('throws when no package name is given', async () => {
    await expect(unstar.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, [])).rejects.toThrow('Package name is required')
  })
})

describe('stars', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('lists the current user stars via the v1 endpoint (array response)', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'GET',
      path: '/-/whoami',
    }).reply(200, JSON.stringify({ username: 'alice' }))
    mockPool.intercept({
      method: 'GET',
      path: '/-/user/v1/star',
    }).reply(200, JSON.stringify(['foo', 'bar']))

    const result = await stars.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, [])

    expect(result).toBe('foo\nbar')
  })

  it('lists a specific user\'s stars', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'GET',
      path: '/-/user/bob/stars',
    }).reply(200, JSON.stringify({ foo: true, bar: true }))

    const result = await stars.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, ['bob'])

    expect(result.split('\n').sort()).toEqual(['bar', 'foo'])
  })

  it('throws when no user is given and the user is not logged in', async () => {
    await expect(stars.handler({
      registries: { default: REGISTRY_URL },
    }, [])).rejects.toThrow('You must be logged in')
  })

  it('throws a helpful error when the user is not found', async () => {
    const mockPool = getMockAgent().get(REGISTRY)
    mockPool.intercept({
      method: 'GET',
      path: '/-/user/bob/stars',
    }).reply(404, '{}')
    mockPool.intercept({
      method: 'GET',
      path: '/-/util/user/bob/stars',
    }).reply(404, '{}')

    await expect(stars.handler({
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY_URL },
    }, ['bob'])).rejects.toThrow('User "bob" not found')
  })
})
