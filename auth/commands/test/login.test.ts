import { describe, expect, it } from '@jest/globals'

import { login, type LoginContext, type Settings } from '../src/login.js'

const DEFAULT_OPTS = {
  configDir: '/mock/config',
  dir: '/mock',
  rawConfig: {},
  registry: 'https://registry.npmjs.org/',
}

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
        opts: DEFAULT_OPTS,
        context: {
          ...TEST_CONTEXT,
          process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
        },
      })
    ).rejects.toThrow('The login command requires an interactive terminal')
  })

  it('should use web login when registry supports it', async () => {
    let savedSettings: Settings = {}

    const result = await login({
      opts: DEFAULT_OPTS,
      context: {
        ...TEST_CONTEXT,
        writeSettings: async (_path, settings) => {
          savedSettings = settings
        },
        fetch: async (url: string) => {
          if (url.includes('/-/v1/login')) {
            return {
              ok: true,
              status: 200,
              json: async () => ({
                loginUrl: 'https://registry.npmjs.org/auth/login',
                doneUrl: 'https://registry.npmjs.org/auth/done',
              }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          if (url.includes('/auth/done')) {
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

    expect(result).toContain('Logged in')
    expect(savedSettings).toMatchObject({
      '//registry.npmjs.org/:_authToken': 'web-auth-token-123',
    })
  })

  it('should fall back to classic login when web login returns 404', async () => {
    let savedSettings: Settings = {}

    const result = await login({
      opts: DEFAULT_OPTS,
      context: {
        ...TEST_CONTEXT,
        writeSettings: async (_path, settings) => {
          savedSettings = settings
        },
        fetch: async (url: string) => {
          if (url.includes('/-/v1/login')) {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          if (url.includes('org.couchdb.user')) {
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

    expect(result).toContain('Logged in')
    expect(savedSettings).toMatchObject({
      '//registry.npmjs.org/:_authToken': 'classic-token-456',
    })
  })
})
