/* cspell:ignore alice bob myscope invalidteam */
import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { access } from '../src/index.js'

const REGISTRY_URL = 'https://registry.npmjs.org/'

describe('access command', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('should have correct command names', () => {
    expect(access.commandNames).toEqual(['access'])
  })

  it('should have a help function', () => {
    const help = access.help()
    expect(help).toContain('Manages package access')
    expect(help).toContain('pnpm access list packages')
    expect(help).toContain('pnpm access grant')
    expect(help).toContain('pnpm access revoke')
  })

  it('should have cliOptionsTypes function', () => {
    const options = access.cliOptionsTypes()
    expect(options).toHaveProperty('registry')
    expect(options).toHaveProperty('json')
    expect(options).toHaveProperty('otp')
  })

  it('should have rcOptionsTypes function', () => {
    const options = access.rcOptionsTypes()
    expect(typeof options).toBe('object')
  })

  it('should throw when no subcommand provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, [])
    }).rejects.toThrow('A subcommand is required')
  })

  it('get status: should get the access status of a scoped package', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
    }).reply(200, { access: 'restricted' })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['get', 'status', '@pnpm/test'])

    expect(result).toContain('package: @pnpm/test')
    expect(result).toContain('access: restricted')
  })

  it('get status: should throw when no package name provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['get', 'status'])
    }).rejects.toThrow('Package name is required')
  })

  it('get status: should throw when package not found', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/package\/nonexistent-pkg-test\/access$/,
    }).reply(404, { error: 'Not found' })

    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['get', 'status', 'nonexistent-pkg-test'])
    }).rejects.toThrow('not found')
  })

  it('set status: should set package to public', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
      body: JSON.stringify({ access: 'public' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['set', 'status=public', '@pnpm/test'])

    expect(result).toBe('@pnpm/test: public')
  })

  it('set status: should set package to private (restricted)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
      body: JSON.stringify({ access: 'restricted' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['set', 'status=private', '@pnpm/test'])

    expect(result).toBe('@pnpm/test: restricted')
  })

  it('set status: should throw when no status value provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set'])
    }).rejects.toThrow('A value is required')
  })

  it('set status: should throw for invalid status value', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set', 'status=invalid', '@pnpm/test'])
    }).rejects.toThrow('Invalid access value')
  })

  it('set status: should throw for unscoped packages', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set', 'status=public', 'unscoped-pkg'])
    }).rejects.toThrow('Unscoped packages are always public')
  })

  it('set status: should throw when no package provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set', 'status=public'])
    }).rejects.toThrow('Package name is required')
  })

  it('set status: should throw on 401 (unauthorized)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
    }).reply(401, { error: 'Unauthorized' })

    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set', 'status=public', '@pnpm/test'])
    }).rejects.toThrow('logged in')
  })

  it('set status: should throw on 403 (forbidden)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
    }).reply(403, { error: 'Forbidden' })

    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set', 'status=public', '@pnpm/test'])
    }).rejects.toThrow('permission')
  })

  it('set mfa: should set MFA to automation', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
      body: JSON.stringify({ publish_requires_tfa: true }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['set', 'mfa=automation', '@pnpm/test'])

    expect(result).toBe('@pnpm/test: mfa=automation')
  })

  it('set mfa: should set MFA to none', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
      body: JSON.stringify({ publish_requires_tfa: false }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['set', 'mfa=none', '@pnpm/test'])

    expect(result).toBe('@pnpm/test: mfa=none')
  })

  it('set mfa: should throw for invalid MFA value', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['set', 'mfa=invalid', '@pnpm/test'])
    }).rejects.toThrow('Invalid MFA value')
  })

  it('grant: should grant read-only access to a team', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: /^\/-\/team\/myscope\/developers\/package$/,
      body: JSON.stringify({ package: '@myscope/pkg', permissions: 'read-only' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['grant', 'read-only', 'myscope:developers', '@myscope/pkg'])

    expect(result).toBe('+myscope:developers (read-only): @myscope/pkg')
  })

  it('grant: should grant read-write access to a team', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'PUT',
      path: /^\/-\/team\/myscope\/developers\/package$/,
      body: JSON.stringify({ package: '@myscope/pkg', permissions: 'read-write' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['grant', 'read-write', 'myscope:developers', '@myscope/pkg'])

    expect(result).toBe('+myscope:developers (read-write): @myscope/pkg')
  })

  it('grant: should throw when permissions and team not provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['grant'])
    }).rejects.toThrow('Permissions and scope:team are required')
  })

  it('grant: should throw for invalid permissions', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['grant', 'invalid', 'myscope:developers', '@myscope/pkg'])
    }).rejects.toThrow('Invalid permissions')
  })

  it('grant: should throw when team format is invalid', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['grant', 'read-only', 'invalidteam', '@myscope/pkg'])
    }).rejects.toThrow('Invalid team')
  })

  it('revoke: should revoke team access', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'DELETE',
      path: /^\/-\/team\/myscope\/developers\/package$/,
      body: JSON.stringify({ package: '@myscope/pkg' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['revoke', 'myscope:developers', '@myscope/pkg'])

    expect(result).toBe('-myscope:developers: @myscope/pkg')
  })

  it('revoke: should throw when no team provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['revoke'])
    }).rejects.toThrow('scope:team and package name are required')
  })

  it('revoke: should throw when team format is invalid', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['revoke', 'invalidteam', '@myscope/pkg'])
    }).rejects.toThrow('Invalid team')
  })

  it('list packages: should list packages for a user', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/user\/alice\/package\?format=cli$/,
    }).reply(200, { '@alice/pkg': 'read-write', '@alice/other': 'read-only' })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['list', 'packages', 'alice'])

    expect(result).toContain('@alice/pkg: read-write')
    expect(result).toContain('@alice/other: read-only')
  })

  it('list packages: should list packages for an org', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/org\/myscope\/package\?format=cli$/,
    }).reply(200, { '@myscope/pkg': 'read-write' })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['list', 'packages', '@myscope'])

    expect(result).toContain('@myscope/pkg: read-write')
  })

  it('list packages: should list packages for a team', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/team\/myscope\/developers\/package\?format=cli$/,
    }).reply(200, { '@myscope/pkg': 'read-write' })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['list', 'packages', 'myscope:developers'])

    expect(result).toContain('@myscope/pkg: read-write')
  })

  it('list collaborators: should list collaborators for a package', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/collaborators\?format=cli$/,
    }).reply(200, [
      { user: 'alice', email: 'alice@example.com', permissions: 'read-write' },
      { user: 'bob', email: 'bob@example.com', permissions: 'read-only' },
    ])

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['list', 'collaborators', '@pnpm/test'])

    expect(result).toContain('alice')
    expect(result).toContain('bob')
    expect(result).toContain('read-write')
    expect(result).toContain('read-only')
  })

  it('list collaborators: should throw when no package provided', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['list', 'collaborators'])
    }).rejects.toThrow('Package name is required')
  })

  it('list collaborators: should filter by user when specified', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/collaborators\?format=cli&user=alice$/,
    }).reply(200, [
      { user: 'alice', email: 'alice@example.com', permissions: 'read-write' },
    ])

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['list', 'collaborators', '@pnpm/test', 'alice'])

    expect(result).toContain('alice')
  })

  it('should support deprecated "public" form', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
      body: JSON.stringify({ access: 'public' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['public', '@pnpm/test'])

    expect(result).toBe('@pnpm/test: public')
  })

  it('should support deprecated "restricted" form', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'POST',
      path: /^\/-\/package\/@pnpm%2[Ff]test\/access$/,
      body: JSON.stringify({ access: 'restricted' }),
    }).reply(200, { ok: true })

    const result = await access.handler({
      cliOptions: {},
      registries: { default: REGISTRY_URL },
    }, ['restricted', '@pnpm/test'])

    expect(result).toBe('@pnpm/test: restricted')
  })

  it('should output JSON when --json flag is set (list packages)', async () => {
    getMockAgent().get('https://registry.npmjs.org').intercept({
      method: 'GET',
      path: /^\/-\/user\/alice\/package\?format=cli$/,
    }).reply(200, { '@alice/pkg': 'read-write' })

    const result = await access.handler({
      cliOptions: { json: true },
      registries: { default: REGISTRY_URL },
    }, ['list', 'packages', 'alice'])

    const parsed = JSON.parse(result)
    expect(parsed).toHaveProperty('@alice/pkg', 'read-write')
  })

  it('should throw on unknown subcommand', async () => {
    await expect(async () => {
      await access.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['unknown'])
    }).rejects.toThrow('Unknown subcommand')
  })
})
