import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { owner } from '../src/index.js'

describe('owner command', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('should have correct command names', () => {
    expect(owner.commandNames).toEqual(['owner', 'owners'])
  })

  it('should have a help function', () => {
    const help = owner.help()
    expect(help).toContain('Manages package owners')
    expect(help).toContain('pnpm owner ls')
  })

  it('should have cliOptionsTypes function', () => {
    const options = owner.cliOptionsTypes()
    expect(options).toHaveProperty('registry')
    expect(options).toHaveProperty('otp')
  })

  it('should have rcOptionsTypes function', () => {
    const options = owner.rcOptionsTypes()
    expect(typeof options).toBe('object')
  })

  it('owner ls: should list owners of a package', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'GET',
      path: '/-/package/%40pnpm%2Ftest/owners',
    }).reply(200, [
      { username: 'alice', email: 'alice@example.com' },
      { username: 'bob', email: 'bob@example.com' },
    ])

    const result = await owner.handler({
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    }, ['ls', '@pnpm/test'])

    expect(result).toContain('alice <alice@example.com>')
    expect(result).toContain('bob <bob@example.com>')
  })

  it('owner ls: should throw when no package name provided', async () => {
    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['ls'])
    }).rejects.toThrow('Package name is required')
  })

  it('owner ls: should throw when package not found', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'GET',
      path: '/-/package/nonexistent-pkg-12345/owners',
    }).reply(404, { error: 'Not found' })

    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['ls', 'nonexistent-pkg-12345'])
    }).rejects.toThrow('not found')
  })

  it('owner add: should add an owner to a package', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'PUT',
      path: '/-/package/%40pnpm%2Ftest/owners',
      body: JSON.stringify({ user: 'newowner' }),
    }).reply(200, { ok: true })

    const result = await owner.handler({
      registry: 'https://registry.npmjs.org/',
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    }, ['add', '@pnpm/test', 'newowner'])

    expect(result).toBe('+newowner: @pnpm/test')
  })

  it('owner add: should throw when package name and owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['add'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner add: should throw when owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['add', '@pnpm/test'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner rm: should remove an owner from a package', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'DELETE',
      path: '/-/package/%40pnpm%2Ftest/owners/newowner',
    }).reply(200, { ok: true })

    const result = await owner.handler({
      registry: 'https://registry.npmjs.org/',
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    }, ['rm', '@pnpm/test', 'newowner'])

    expect(result).toBe('-newowner: @pnpm/test')
  })

  it('owner rm: should throw when package name and owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['rm'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner rm: should throw when owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['rm', '@pnpm/test'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner add: should throw on 401 (unauthorized)', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'PUT',
      path: '/-/package/%40pnpm%2Ftest/owners',
    }).reply(401, { error: 'Unauthorized' })

    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['add', '@pnpm/test', 'newowner'])
    }).rejects.toThrow('UNAUTHORIZED')
  })

  it('owner add: should throw on 403 (forbidden)', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'PUT',
      path: '/-/package/%40pnpm%2Ftest/owners',
    }).reply(403, { error: 'Forbidden' })

    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['add', '@pnpm/test', 'newowner'])
    }).rejects.toThrow('FORBIDDEN')
  })

  it('owner rm: should throw on 404 (package not found)', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'DELETE',
      path: '/-/package/nonexistent-pkg-12345/owners/oldowner',
    }).reply(404, { error: 'Not found' })

    await expect(async () => {
      await owner.handler({
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['rm', 'nonexistent-pkg-12345', 'oldowner'])
    }).rejects.toThrow('not found')
  })

  it('owner ls: should default to ls when no subcommand given', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'GET',
      path: '/-/package/%40pnpm%2Ftest/owners',
    }).reply(200, [
      { username: 'alice', email: 'alice@example.com' },
    ])

    const result = await owner.handler({
      registry: 'https://registry.npmjs.org/',
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    }, ['@pnpm/test'])

    expect(result).toContain('alice <alice@example.com>')
  })

  it('owner: should accept "list" as alias for ls', async () => {
    const mockPool = getMockAgent().get('https://registry.npmjs.org')
    mockPool.intercept({
      method: 'GET',
      path: '/-/package/%40pnpm%2Ftest/owners',
    }).reply(200, [
      { username: 'alice', email: 'alice@example.com' },
    ])

    const result = await owner.handler({
      registry: 'https://registry.npmjs.org/',
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    }, ['list', '@pnpm/test'])

    expect(result).toContain('alice <alice@example.com>')
  })
})