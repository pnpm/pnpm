import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { team } from '../src/index.js'

const REGISTRY_URL = 'https://registry.npmjs.org/'

describe('team command', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  it('should have correct command names', () => {
    expect(team.commandNames).toEqual(['team'])
  })

  it('should have a help function', () => {
    const help = team.help()
    expect(help).toContain('Manage organization teams')
    expect(help).toContain('pnpm team create')
  })

  it('should have cliOptionsTypes function', () => {
    const options = team.cliOptionsTypes()
    expect(options).toHaveProperty('registry')
    expect(options).toHaveProperty('otp')
    expect(options).toHaveProperty('parseable')
    expect(options).toHaveProperty('json')
  })

  it('should have rcOptionsTypes function', () => {
    const options = team.rcOptionsTypes()
    expect(typeof options).toBe('object')
  })

  describe('create', () => {
    it('should create a new team', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'PUT',
        path: '/-/org/myorg/team',
        body: JSON.stringify({ name: 'newteam' }),
      }).reply(200, { ok: true })

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['create', '@myorg:newteam'])

      expect(result).toBe('+myorg:newteam')
    })

    it('should throw when no scope provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['create'])
      }).rejects.toThrow('Team scope is required')
    })

    it('should throw when no team name provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['create', '@myorg'])
      }).rejects.toThrow('Team name is required')
    })

    it('should pass otp header when provided', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'PUT',
        path: '/-/org/myorg/team',
      }).reply(200, { ok: true })

      const result = await team.handler({
        cliOptions: { otp: '123456' },
        registries: { default: REGISTRY_URL },
      }, ['create', '@myorg:newteam'])

      expect(result).toBe('+myorg:newteam')
    })

    it('should throw on 401 (unauthorized)', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'PUT',
        path: '/-/org/myorg/team',
      }).reply(401, { error: 'Unauthorized' })

      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['create', '@myorg:newteam'])
      }).rejects.toThrow('logged in')
    })

    it('should throw on 403 (forbidden)', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'PUT',
        path: '/-/org/myorg/team',
      }).reply(403, { error: 'Forbidden' })

      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['create', '@myorg:newteam'])
      }).rejects.toThrow('permission')
    })

    it('should throw on empty team name after colon', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['create', '@myorg:'])
      }).rejects.toThrow('Team spec must start with @scope')
    })
  })

  describe('destroy', () => {
    it('should destroy a team', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'DELETE',
        path: '/-/team/myorg/oldteam',
      }).reply(200, { ok: true })

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['destroy', '@myorg:oldteam'])

      expect(result).toBe('-myorg:oldteam')
    })

    it('should throw when no scope provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['destroy'])
      }).rejects.toThrow('Team scope is required')
    })

    it('should throw when no team name provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['destroy', '@myorg'])
      }).rejects.toThrow('Team name is required')
    })
  })

  describe('add', () => {
    it('should add a user to a team', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'PUT',
        path: '/-/team/myorg/team1/user',
        body: JSON.stringify({ user: 'alice' }),
      }).reply(200, { ok: true })

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['add', '@myorg:team1', 'alice'])

      expect(result).toBe('+alice added to @myorg:team1')
    })

    it('should throw when team scope and user not provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['add'])
      }).rejects.toThrow('Team scope and user are required')
    })

    it('should throw when user not provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['add', '@myorg:team1'])
      }).rejects.toThrow('Team scope and user are required')
    })

    it('should throw when only scope given (no team name)', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['add', '@myorg', 'alice'])
      }).rejects.toThrow('Team name is required')
    })
  })

  describe('rm', () => {
    it('should remove a user from a team', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'DELETE',
        path: '/-/team/myorg/team1/user',
        body: JSON.stringify({ user: 'bob' }),
      }).reply(200, { ok: true })

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['rm', '@myorg:team1', 'bob'])

      expect(result).toBe('-bob removed from @myorg:team1')
    })

    it('should throw when team scope and user not provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['rm'])
      }).rejects.toThrow('Team scope and user are required')
    })

    it('should throw when user not provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['rm', '@myorg:team1'])
      }).rejects.toThrow('Team scope and user are required')
    })
  })

  describe('ls', () => {
    it('should list teams in an org', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [
        { name: 'developers' },
        { name: 'admins' },
      ])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg'])

      expect(result).toContain('@myorg has the following teams:')
      expect(result).toContain('@myorg:developers')
      expect(result).toContain('@myorg:admins')
    })

    it('should list members of a team', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/team/myorg/developers/user',
      }).reply(200, [
        { name: 'alice' },
        { name: 'bob' },
      ])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg:developers'])

      expect(result).toContain('@myorg:developers has the following members:')
      expect(result).toContain('alice')
      expect(result).toContain('bob')
    })

    it('should list teams with parseable output', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [
        { name: 'developers' },
        { name: 'admins' },
      ])

      const result = await team.handler({
        cliOptions: { parseable: true },
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg'])

      expect(result).toBe('developers\nadmins')
    })

    it('should list teams with json output', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [
        { name: 'developers' },
        { name: 'admins' },
      ])

      const result = await team.handler({
        cliOptions: { json: true },
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg'])

      expect(result).toContain('"developers"')
      expect(result).toContain('"admins"')
    })

    it('should report empty teams', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg'])

      expect(result).toBe('@myorg has no teams')
    })

    it('should report empty team members', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/team/myorg/empty-team/user',
      }).reply(200, [])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg:empty-team'])

      expect(result).toBe('@myorg:empty-team has no members')
    })

    it('should throw when org not found', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/nonexistent/team',
      }).reply(404, { error: 'Not found' })

      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['ls', '@nonexistent'])
      }).rejects.toThrow('not found')
    })

    it('should throw when team not found', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/team/myorg/nonexistent/user',
      }).reply(404, { error: 'Not found' })

      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['ls', '@myorg:nonexistent'])
      }).rejects.toThrow('not found')
    })

    it('should throw when no scope provided', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['ls'])
      }).rejects.toThrow('Organization scope is required')
    })

    it('should accept "list" as alias for ls', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [{ name: 'developers' }])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['list', '@myorg'])

      expect(result).toContain('@myorg:developers')
    })
  })

  describe('default behavior (no subcommand)', () => {
    it('should assume ls when given a scope', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [{ name: 'developers' }])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['@myorg'])

      expect(result).toContain('@myorg:developers')
    })

    it('should assume ls when given a scope:team', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/team/myorg/developers/user',
      }).reply(200, [{ name: 'alice' }])

      const result = await team.handler({
        cliOptions: {},
        registries: { default: REGISTRY_URL },
      }, ['@myorg:developers'])

      expect(result).toContain('alice')
    })

    it('should throw when first arg does not start with @', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['invalid'])
      }).rejects.toThrow('Subcommand is required')
    })
  })

  describe('errors', () => {
    it('should throw on invalid scope spec', async () => {
      await expect(async () => {
        await team.handler({
          cliOptions: {},
          registries: { default: REGISTRY_URL },
        }, ['create', 'invalid'])
      }).rejects.toThrow('Team spec must start with @scope')
    })

    it('should throw on inappropriate parseable ls with just scope', async () => {
      getMockAgent().get('https://registry.npmjs.org').intercept({
        method: 'GET',
        path: '/-/org/myorg/team',
      }).reply(200, [{ name: 'developers' }])

      const result = await team.handler({
        cliOptions: { parseable: true },
        registries: { default: REGISTRY_URL },
      }, ['ls', '@myorg'])

      expect(result).toBe('developers')
    })
  })
})
