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
  safeReadIniFile: async (path) => {
    throw new Error(`unexpected safeReadIniFile call: ${path}`)
  },
  writeIniFile: async (path) => {
    throw new Error(`unexpected writeIniFile call: ${path}`)
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
        safeReadIniFile: async () => ({}),
        writeIniFile: async (configPath, settings) => {
          savedPath = configPath
          savedSettings = settings
        },
        fetch: async (url) => {
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
        safeReadIniFile: async () => ({}),
        writeIniFile: async (configPath, settings) => {
          savedPath = configPath
          savedSettings = settings
        },
        fetch: async (url) => {
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

  it('should handle classic OTP challenge during login', async () => {
    let putCallCount = 0
    const infoMessages: string[] = []

    const result = await login({
      opts: {
        configDir: '/otp/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: (message) => {
          infoMessages.push(message)
        },
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url, options) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.org/-/user/org.couchdb.user:alice') {
            putCallCount++
            if (putCallCount === 1) {
              return {
                ok: false,
                status: 401,
                json: async () => ({ error: 'otp required' }),
                text: async () => 'OTP required',
                headers: { get: (name: string) => name === 'www-authenticate' ? 'OTP otp' : null },
              }
            }
            // Second call should include npm-otp header
            expect(options?.headers?.['npm-otp']).toBe('999999')
            return {
              ok: true,
              status: 201,
              json: async () => ({ ok: true, token: 'otp-token-789' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'alice' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'alice@example.com' }
            if (opts.name === 'otp') return { otp: '999999' }
            throw new Error(`unexpected prompt call: ${opts.name}`)
          },
        },
      },
    })

    expect(result).toBe('Logged in on https://example.org/')
    expect(putCallCount).toBe(2)
  })

  it('should handle webauth OTP challenge during login', async () => {
    let putCallCount = 0
    let pollCallCount = 0
    const infoMessages: string[] = []

    const result = await login({
      opts: {
        configDir: '/otp/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: (message) => {
          infoMessages.push(message)
        },
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url, options) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.org/-/user/org.couchdb.user:bob') {
            putCallCount++
            if (putCallCount === 1) {
              return {
                ok: false,
                status: 401,
                json: async () => ({
                  authUrl: 'https://example.org/auth/web',
                  doneUrl: 'https://example.org/auth/web/done',
                }),
                text: async () => '',
                headers: { get: (name: string) => name === 'www-authenticate' ? 'OTP otp' : null },
              }
            }
            expect(options?.headers?.['npm-otp']).toBe('web-tok')
            return {
              ok: true,
              status: 201,
              json: async () => ({ ok: true, token: 'final-token' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.org/auth/web/done') {
            pollCallCount++
            return {
              ok: true,
              status: 200,
              json: async () => ({ token: 'web-tok' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'bob' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'bob@example.com' }
            throw new Error(`unexpected prompt call: ${opts.name}`)
          },
        },
      },
    })

    expect(result).toBe('Logged in on https://example.org/')
    expect(putCallCount).toBe(2)
    expect(pollCallCount).toBe(1)
    // Should have shown the auth URL and QR code
    expect(infoMessages).toContainEqual(expect.stringMatching(/(?:^|\s)https:\/\/example\.org\/auth\/web(?:\s|$)/))
  })

  it('should not trigger OTP for non-401 errors', async () => {
    await expect(login({
      opts: {
        configDir: '/otp/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: () => {},
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          // Return 403 (not 401) — should not trigger OTP
          return {
            ok: false,
            status: 403,
            json: async () => ({}),
            text: async () => 'Forbidden',
            headers: { get: () => null },
          }
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'alice' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'alice@example.com' }
            throw new Error('should not prompt for OTP')
          },
        },
      },
    })).rejects.toThrow('Login failed (HTTP 403): Forbidden')
  })

  it('should throw when username is empty in classic login', async () => {
    await expect(login({
      opts: {
        configDir: '/mock/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: () => {},
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: '' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'a@b.com' }
            throw new Error(`unexpected prompt call: ${opts.name}`)
          },
        },
      },
    })).rejects.toThrow('Username, password, and email are all required')
  })

  it('should throw when classic login returns no token', async () => {
    await expect(login({
      opts: {
        configDir: '/mock/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: () => {},
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.org/-/user/org.couchdb.user:alice') {
            return {
              ok: true,
              status: 201,
              json: async () => ({ ok: true }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'alice' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'alice@example.com' }
            throw new Error(`unexpected prompt call: ${opts.name}`)
          },
        },
      },
    })).rejects.toThrow('The registry did not return an authentication token')
  })

  it('should throw when web login returns invalid response (missing loginUrl/doneUrl)', async () => {
    await expect(login({
      opts: {
        configDir: '/mock/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: () => {},
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: true,
              status: 200,
              json: async () => ({ loginUrl: 'https://example.org/auth' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
      },
    })).rejects.toThrow('The registry returned an invalid response for web-based login')
  })

  it('should fall back to classic login when web login returns 405', async () => {
    let savedSettings: Settings = {}

    const result = await login({
      opts: {
        configDir: '/mock/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: () => {},
        safeReadIniFile: async () => ({}),
        writeIniFile: async (_configPath, settings) => {
          savedSettings = settings
        },
        fetch: async (url) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 405,
              json: async () => ({}),
              text: async () => 'Method Not Allowed',
              headers: { get: () => null },
            }
          }
          if (url === 'https://example.org/-/user/org.couchdb.user:jane') {
            return {
              ok: true,
              status: 201,
              json: async () => ({ ok: true, token: 'token-405' }),
              text: async () => '',
              headers: { get: () => null },
            }
          }
          throw new Error(`unexpected fetch call: ${url}`)
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'jane' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'jane@example.com' }
            throw new Error(`unexpected prompt call: ${opts.name}`)
          },
        },
      },
    })

    expect(result).toBe('Logged in on https://example.org/')
    expect(savedSettings).toMatchObject({
      '//example.org/:_authToken': 'token-405',
    })
  })

  it('should not trigger OTP for 401 without www-authenticate otp header', async () => {
    await expect(login({
      opts: {
        configDir: '/otp/config',
        dir: '/mock',
        rawConfig: {},
        registry: 'https://example.org',
      },
      context: {
        ...TEST_CONTEXT,
        globalInfo: () => {},
        safeReadIniFile: async () => ({}),
        writeIniFile: async () => {},
        fetch: async (url) => {
          if (url === 'https://example.org/-/v1/login') {
            return {
              ok: false,
              status: 404,
              json: async () => ({}),
              text: async () => 'Not Found',
              headers: { get: () => null },
            }
          }
          // Return 401 but without www-authenticate: otp header
          return {
            ok: false,
            status: 401,
            json: async () => ({}),
            text: async () => 'Unauthorized',
            headers: { get: () => null },
          }
        },
        enquirer: {
          prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
            if (opts.name === 'username') return { username: 'alice' }
            if (opts.name === 'password') return { password: 'pass' }
            if (opts.name === 'email') return { email: 'alice@example.com' }
            throw new Error('should not prompt for OTP')
          },
        },
      },
    })).rejects.toThrow('Login failed (HTTP 401): Unauthorized')
  })
})
