import { describe, expect, it } from '@jest/globals'

import { login, type LoginContext, type Settings } from '../src/login.js'

const TEST_CONTEXT: LoginContext = {
  Date,
  setTimeout: (cb) => {
    cb()
  },
  enquirer: { prompt: async () => ({}) },
  fetch: async () => ({ ok: false, status: 500, json: async () => ({}), text: async () => '', headers: { get: () => null } }),
  globalInfo: () => {},
  process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
  readSettings: async () => ({}),
  writeSettings: async () => {},
}

describe('login', () => {
  it('should throw in non-interactive terminal', async () => {
    await expect(
      login({
        opts: {
          configDir: '/mock/config',
          dir: '/mock',
          rawConfig: {},
        },
        context: {
          ...TEST_CONTEXT,
          process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
        },
      })
    ).rejects.toThrow('The login command requires an interactive terminal')
  })

  it('should use web login when registry supports it', async () => {
    const fetchedUrls: string[] = []
    let savedPath = ''
    let savedSettings: Settings = {}

    const result = await login({
      opts: {
        configDir: '/custom/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://custom.registry.io/npm/',
      },
      context: {
        ...TEST_CONTEXT,
        writeSettings: async (configPath, settings) => {
          savedPath = configPath
          savedSettings = settings
        },
        fetch: async (url: string) => {
          fetchedUrls.push(url)
          if (url === 'https://custom.registry.io/npm/-/v1/login') {
            return {
              ok: true,
              status: 200,
              json: async () => ({
                loginUrl: 'https://custom.registry.io/auth/login',
                doneUrl: 'https://custom.registry.io/auth/done',
              }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          if (url === 'https://custom.registry.io/auth/done') {
            return {
              ok: true,
              status: 200,
              json: async () => ({ token: 'web-auth-token-123' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          return { ok: false, status: 404, json: async () => ({}), text: async () => '', headers: { get: () => null } }
        },
      },
    })

    expect(result).toBe('Logged in on https://custom.registry.io/npm/')
    expect(fetchedUrls[0]).toBe('https://custom.registry.io/npm/-/v1/login')
    expect(savedPath).toBe('/custom/config/rc')
    expect(savedSettings).toMatchObject({
      '//custom.registry.io/npm/:_authToken': 'web-auth-token-123',
    })
  })

  it('should fall back to classic login when web login returns 404', async () => {
    const fetchedUrls: string[] = []
    let savedPath = ''
    let savedSettings: Settings = {}

    const result = await login({
      opts: {
        configDir: '/other/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://private.reg.co',
      },
      context: {
        ...TEST_CONTEXT,
        writeSettings: async (configPath, settings) => {
          savedPath = configPath
          savedSettings = settings
        },
        fetch: async (url: string) => {
          fetchedUrls.push(url)
          if (url === 'https://private.reg.co/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          if (url === 'https://private.reg.co/-/user/org.couchdb.user:john') {
            return {
              ok: true,
              status: 201,
              json: async () => ({ ok: true, token: 'classic-token-456' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          return { ok: false, status: 500, json: async () => ({}), text: async () => '', headers: { get: () => null } }
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'john' }
            if (opts.name === 'password') return { password: 'secret' }
            if (opts.name === 'email') return { email: 'john@example.com' }
            return {}
          },
        },
      },
    })

    expect(result).toBe('Logged in on https://private.reg.co/')
    expect(fetchedUrls[0]).toBe('https://private.reg.co/-/v1/login')
    expect(fetchedUrls[1]).toBe('https://private.reg.co/-/user/org.couchdb.user:john')
    expect(savedPath).toBe('/other/config/rc')
    expect(savedSettings).toMatchObject({
      '//private.reg.co/:_authToken': 'classic-token-456',
    })
  })
})
