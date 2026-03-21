import { describe, expect, it } from '@jest/globals'

import { login, type LoginContext, type Settings } from '../src/login.js'

const TEST_CONTEXT: LoginContext = {
  Date: { now: () => 0 },
  setTimeout: (cb) => {
    cb()
  },
  enquirer: { prompt: async () => {
    throw new Error('unexpected prompt call')
  } },
  fetch: async (url) => {
    throw new Error(`unexpected fetch call: ${url}`)
  },
  globalInfo: (message) => {
    throw new Error(`unexpected globalInfo call: ${message}`)
  },
  process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
  readSettings: async (path) => {
    throw new Error(`unexpected readSettings call: ${path}`)
  },
  writeSettings: async (path) => {
    throw new Error(`unexpected writeSettings call: ${path}`)
  },
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
    const infoMessages: string[] = []
    let savedPath = ''
    let savedSettings: Settings = {}

    const result = await login({
      opts: {
        configDir: '/custom/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.com/npm/',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: (message) => {
          infoMessages.push(message)
        },
        readSettings: async () => ({}),
        writeSettings: async (configPath, settings) => {
          savedPath = configPath
          savedSettings = settings
        },
        fetch: async (url: string) => {
          fetchedUrls.push(url)
          if (url === 'https://example.com/npm/-/v1/login') {
            return {
              ok: true,
              status: 200,
              json: async () => ({
                loginUrl: 'https://example.com/auth/login',
                doneUrl: 'https://example.com/auth/done',
              }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.com/auth/done') {
            return {
              ok: true,
              status: 200,
              json: async () => ({ token: 'web-auth-token-123' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
      },
    })

    expect(result).toBe('Logged in on https://example.com/npm/')
    expect(fetchedUrls[0]).toBe('https://example.com/npm/-/v1/login')
    expect(savedPath).toBe('/custom/config/rc')
    expect(savedSettings).toMatchObject({
      '//example.com/npm/:_authToken': 'web-auth-token-123',
    })
    expect(infoMessages).toHaveLength(1)
    expect(infoMessages[0]).toContain('https://example.com/auth/login')
  })

  it('should fall back to classic login when web login returns 404', async () => {
    const fetchedUrls: string[] = []
    const infoMessages: string[] = []
    let savedPath = ''
    let savedSettings: Settings = {}

    const result = await login({
      opts: {
        configDir: '/other/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: (message) => {
          infoMessages.push(message)
        },
        readSettings: async () => ({}),
        writeSettings: async (configPath, settings) => {
          savedPath = configPath
          savedSettings = settings
        },
        fetch: async (url: string) => {
          fetchedUrls.push(url)
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.org/-/user/org.couchdb.user:john') {
            return {
              ok: true,
              status: 201,
              json: async () => ({ ok: true, token: 'classic-token-456' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'john' }
            if (opts.name === 'password') return { password: 'secret' }
            if (opts.name === 'email') return { email: 'john@example.com' }
            throw new Error(`unexpected prompt call: ${opts.name}`)
          },
        },
      },
    })

    expect(result).toBe('Logged in on https://example.org/')
    expect(fetchedUrls[0]).toBe('https://example.org/-/v1/login')
    expect(fetchedUrls[1]).toBe('https://example.org/-/user/org.couchdb.user:john')
    expect(savedPath).toBe('/other/config/rc')
    expect(savedSettings).toMatchObject({
      '//example.org/:_authToken': 'classic-token-456',
    })
    expect(infoMessages).toEqual(['Logged in as john'])
  })
})
