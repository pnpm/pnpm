/* cspell:ignore Ftest newowner oldowner */
import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { owner } from '../src/index.js'

const REGISTRY_URL = 'https://registry.npmjs.org/'

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

  it('owner ls: should list maintainers from package metadata', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/@pnpm%2[Ff]test$/,
      headers: { accept: 'application/json; q=1.0, */*' },
    }).reply(200, {
      maintainers: [
        { name: 'alice', email: 'alice@example.com' },
        { name: 'bob', email: 'bob@example.com' },
      ],
    })

    const result = await owner.handler({
      cliOptions: {},
      registriesByScope: {
        default: REGISTRY_URL,
      },
    }, ['ls', '@pnpm/test'])

    expect(result).toContain('alice <alice@example.com>')
    expect(result).toContain('bob <bob@example.com>')
  })

  it.each([{}, { maintainers: [] }, { maintainers: null }])('owner ls: should accept metadata without owners: %j', async (metadata) => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: '/test-package',
    }).reply(200, metadata)

    expect(await owner.handler({}, ['ls', 'test-package'])).toBe('')
  })

  it('owner ls: should accept maintainers without an email', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: '/test-package',
    }).reply(200, { maintainers: [{ name: 'alice' }] })

    expect(await owner.handler({}, ['ls', 'test-package'])).toBe('alice')
  })

  it('owner ls: should preserve scoped registry authentication and path prefixes', async () => {
    getMockAgent().get('https://registry.example.com').intercept({
      method: 'GET',
      path: /^\/npm\/@pnpm%2[Ff]test$/,
      headers: { authorization: 'Bearer test-token', accept: 'application/json; q=1.0, */*' },
    }).reply(200, { maintainers: [{ name: 'alice', email: 'alice@example.com' }] })

    const result = await owner.handler({
      registriesByScope: {
        default: REGISTRY_URL,
        '@pnpm': 'https://registry.example.com/npm/',
      },
      configByUri: {
        '//registry.example.com/npm/': { '@': { authToken: 'test-token' } },
      },
    }, ['ls', '@pnpm/test'])

    expect(result).toBe('alice <alice@example.com>')
  })

  it('owner ls: should read metadata larger than 1 MiB', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: '/test-package',
    }).reply(200, {
      readme: 'x'.repeat(1024 * 1024),
      maintainers: [{ name: 'alice', email: 'alice@example.com' }],
    })

    expect(await owner.handler({}, ['ls', 'test-package'])).toBe('alice <alice@example.com>')
  })

  it('owner ls: should throw when no package name provided', async () => {
    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['ls'])
    }).rejects.toThrow('Package name is required')
  })

  it('owner ls: should throw when package not found', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: '/nonexistent-pkg-12345',
    }).reply(404, { error: 'Not found' })

    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['ls', 'nonexistent-pkg-12345'])
    }).rejects.toThrow('not found')
  })

  it('owner ls: should throw on 401 (unauthorized)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/@pnpm%2[Ff]test$/,
    }).reply(401, { error: 'Unauthorized' })

    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['ls', '@pnpm/test'])
    }).rejects.toThrow('logged in')
  })

  it('owner ls: should throw on 403 (forbidden)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/@pnpm%2[Ff]test$/,
    }).reply(403, { error: 'Forbidden' })

    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['ls', '@pnpm/test'])
    }).rejects.toThrow('permission')
  })

  it('owner add: should add an owner to a package', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/owners$/,
      body: JSON.stringify({ user: 'newowner' }),
    }).reply(200, { ok: true })

    const result = await owner.handler({
      cliOptions: {},
      registriesByScope: {
        default: REGISTRY_URL,
      },
    }, ['add', '@pnpm/test', 'newowner'])

    expect(result).toBe('+newowner: @pnpm/test')
  })

  it('owner add: should throw when package name and owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['add'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner add: should throw when owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['add', '@pnpm/test'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner rm: should remove an owner from a package', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'DELETE',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/owners\/newowner$/,
    }).reply(200, { ok: true })

    const result = await owner.handler({
      cliOptions: {},
      registriesByScope: {
        default: REGISTRY_URL,
      },
    }, ['rm', '@pnpm/test', 'newowner'])

    expect(result).toBe('-newowner: @pnpm/test')
  })

  it('owner rm: should throw when package name and owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['rm'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner rm: should throw when owner not provided', async () => {
    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['rm', '@pnpm/test'])
    }).rejects.toThrow('Package name and owner are required')
  })

  it('owner add: should throw on 401 (unauthorized)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/owners$/,
    }).reply(401, { error: 'Unauthorized' })

    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['add', '@pnpm/test', 'newowner'])
    }).rejects.toThrow('logged in')
  })

  it('owner add: should throw on 403 (forbidden)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/owners$/,
    }).reply(403, { error: 'Forbidden' })

    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: 'https://registry.npmjs.org/',
        },
      }, ['add', '@pnpm/test', 'newowner'])
    }).rejects.toThrow('permission')
  })

  it('owner rm: should throw on 404 (package not found)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'DELETE',
      path: /^\/-\/package\/nonexistent-pkg-12345\/owners\/oldowner$/,
    }).reply(404, { error: 'Not found' })

    await expect(async () => {
      await owner.handler({
        cliOptions: {},
        registriesByScope: {
          default: REGISTRY_URL,
        },
      }, ['rm', 'nonexistent-pkg-12345', 'oldowner'])
    }).rejects.toThrow('not found')
  })

  it('owner ls: should default to ls when no subcommand given', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: '/test-package',
    }).reply(200, {
      maintainers: [{ name: 'alice', email: 'alice@example.com' }],
    })

    const result = await owner.handler({
      cliOptions: {},
      registriesByScope: {
        default: REGISTRY_URL,
      },
    }, ['test-package'])

    expect(result).toContain('alice <alice@example.com>')
  })

  it('owner: should accept "list" as alias for ls', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/@pnpm%2[Ff]test$/,
    }).reply(200, {
      maintainers: [{ name: 'alice', email: 'alice@example.com' }],
    })

    const result = await owner.handler({
      cliOptions: {},
      registriesByScope: {
        default: REGISTRY_URL,
      },
    }, ['list', '@pnpm/test'])

    expect(result).toContain('alice <alice@example.com>')
  })
})