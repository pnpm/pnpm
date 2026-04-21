import path from 'node:path'

import { describe, expect, it, jest } from '@jest/globals'

import { login, type LoginContext, type LoginFetchResponse } from '../src/login.js'

const TEST_CONTEXT: LoginContext = {
  Date: { now: () => 0 },
  setTimeout: cb => {
    cb()
  },
  createReadlineInterface: () => ({
    once: () => {},
    close: () => {},
  }),
  enquirer: { prompt: async () => {
    throw new Error('Unexpected call to enquirer.prompt')
  } },
  fetch: async url => {
    throw new Error(`Unexpected call to fetch: ${url}`)
  },
  globalInfo: message => {
    throw new Error(`Unexpected call to globalInfo: ${message}`)
  },
  globalWarn: message => {
    throw new Error(`Unexpected call to globalWarn: ${message}`)
  },
  process: {
    platform: 'linux',
    stdin: { isTTY: true },
    stdout: { isTTY: true },
  },
  readIniFile: async path => {
    throw new Error(`Unexpected call to readIniFile: ${path}`)
  },
  writeIniFile: async path => {
    throw new Error(`Unexpected call to writeIniFile: ${path}`)
  },
}

const createMockResponse = (init: {
  ok: boolean
  status: number
  json?: unknown
  text?: string
  headers?: LoginFetchResponse['headers']
}): LoginFetchResponse => {
  let bodyConsumed = false
  return {
    ok: init.ok,
    status: init.status,
    json: async () => {
      if (bodyConsumed) throw new Error('Unexpected double consumption of response body')
      bodyConsumed = true
      return init.json ?? {}
    },
    text: async () => {
      if (bodyConsumed) throw new Error('Unexpected double consumption of response body')
      bodyConsumed = true
      return init.text ?? ''
    },
    headers: init.headers ?? {
      get: name => {
        throw new Error(`Unexpected call to headers.get: ${name}`)
      },
    },
  }
}

type MockContextOverrides = Omit<Partial<LoginContext>, 'process'> & {
  process?: Partial<LoginContext['process']>
}

const createMockContext = (overrides?: MockContextOverrides): LoginContext => ({
  ...TEST_CONTEXT,
  ...overrides,
  process: {
    ...TEST_CONTEXT.process,
    ...overrides?.process,
  },
})

describe('login', () => {
  it('should throw in non-interactive terminal', async () => {
    const context = createMockContext({
      process: {
        stdin: { isTTY: false },
      },
    })
    const opts = { configDir: '/mock/config', dir: '/mock', authConfig: {} }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_LOGIN_NON_INTERACTIVE')
    await expect(promise).rejects.toHaveProperty(['message'], 'The login command requires an interactive terminal')
  })

  it('should use web login when registry supports it', async () => {
    const fetchedUrls: string[] = []
    const globalInfo = jest.fn()
    let savedPath = ''
    let savedSettings: Record<string, unknown> = {}
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => ({}),
      writeIniFile: async (configPath, settings) => {
        savedPath = configPath
        savedSettings = settings
      },
      fetch: async url => {
        fetchedUrls.push(url)
        if (url === 'https://example.com/npm/-/v1/login') {
          return createMockResponse({
            ok: true,
            status: 200,
            json: {
              loginUrl: 'https://example.com/auth/login',
              doneUrl: 'https://example.com/auth/done',
            },
          })
        }
        if (url === 'https://example.com/auth/done') {
          return createMockResponse({
            ok: true,
            status: 200,
            json: { token: 'web-auth-token-123' },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
    })
    const opts = { configDir: '/custom/config', dir: '/mock', authConfig: {}, registry: 'https://example.com/npm/' }
    const result = await login({ context, opts })
    expect(result).toBe('Logged in on https://example.com/npm/')
    expect(fetchedUrls[0]).toBe('https://example.com/npm/-/v1/login')
    expect(savedPath).toBe(path.join('/custom/config', 'auth.ini'))
    expect(savedSettings).toMatchObject({
      '//example.com/npm/:_authToken': 'web-auth-token-123',
    })
    expect(globalInfo.mock.calls).toEqual([
      [expect.stringContaining('https://example.com/auth/login')],
      ['Press ENTER to open the URL in your browser.'],
    ])
  })

  it('should fall back to classic login when web login returns 404', async () => {
    const fetchedUrls: string[] = []
    const globalInfo = jest.fn()
    let savedPath = ''
    let savedSettings: Record<string, unknown> = {}
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => ({}),
      writeIniFile: async (configPath, settings) => {
        savedPath = configPath
        savedSettings = settings
      },
      fetch: async url => {
        fetchedUrls.push(url)
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        if (url === 'https://example.org/-/user/org.couchdb.user:john') {
          return createMockResponse({
            ok: true,
            status: 201,
            json: { ok: true, token: 'classic-token-456' },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'john' }
          if (opts.name === 'password') return { password: 'secret' }
          if (opts.name === 'email') return { email: 'john@example.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/other/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const result = await login({ context, opts })
    expect(result).toBe('Logged in on https://example.org/')
    expect(fetchedUrls[0]).toBe('https://example.org/-/v1/login')
    expect(fetchedUrls[1]).toBe('https://example.org/-/user/org.couchdb.user:john')
    expect(savedPath).toBe(path.join('/other/config', 'auth.ini'))
    expect(savedSettings).toMatchObject({
      '//example.org/:_authToken': 'classic-token-456',
    })
    expect(globalInfo.mock.calls).toEqual([['Logged in as john']])
  })

  it('should handle classic OTP challenge during login', async () => {
    let putCallCount = 0
    const globalInfo = jest.fn()
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async (url, options) => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        if (url === 'https://example.org/-/user/org.couchdb.user:alice') {
          putCallCount++
          if (putCallCount === 1) {
            return createMockResponse({
              ok: false,
              status: 401,
              json: { error: 'otp required' },
              text: 'OTP required',
              headers: { get: (name: string) => name === 'www-authenticate' ? 'OTP otp' : null },
            })
          }
          expect(options?.headers?.['npm-otp']).toBe('999999')
          return createMockResponse({
            ok: true,
            status: 201,
            json: { ok: true, token: 'otp-token-789' },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'alice' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'alice@example.com' }
          if (opts.name === 'otp') return { otp: '999999' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/otp/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const result = await login({ context, opts })
    expect(result).toBe('Logged in on https://example.org/')
    expect(putCallCount).toBe(2)
    expect(globalInfo.mock.calls).toEqual([['Logged in as alice']])
  })

  it('should handle webauth OTP challenge during login', async () => {
    let putCallCount = 0
    let pollCallCount = 0
    const globalInfo = jest.fn()
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async (url, options) => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        if (url === 'https://example.org/-/user/org.couchdb.user:bob') {
          putCallCount++
          if (putCallCount === 1) {
            return createMockResponse({
              ok: false,
              status: 401,
              json: {
                authUrl: 'https://example.org/auth/web',
                doneUrl: 'https://example.org/auth/web/done',
              },
              headers: { get: (name: string) => name === 'www-authenticate' ? 'OTP otp' : null },
            })
          }
          expect(options?.headers?.['npm-otp']).toBe('web-tok')
          return createMockResponse({
            ok: true,
            status: 201,
            json: { ok: true, token: 'final-token' },
          })
        }
        if (url === 'https://example.org/auth/web/done') {
          pollCallCount++
          return createMockResponse({
            ok: true,
            status: 200,
            json: { token: 'web-tok' },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'bob' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'bob@example.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/otp/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const result = await login({ context, opts })
    expect(result).toBe('Logged in on https://example.org/')
    expect(putCallCount).toBe(2)
    expect(pollCallCount).toBe(1)
    expect(globalInfo.mock.calls).toContainEqual([expect.stringMatching(/(?:^|\s)https:\/\/example\.org\/auth\/web(?:\s|$)/)])
  })

  it('should not trigger OTP for non-401 errors', async () => {
    const context = createMockContext({
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        // Return 403 (not 401) — should not trigger OTP
        return createMockResponse({
          ok: false,
          status: 403,
          text: 'Forbidden',
        })
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'alice' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'alice@example.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/otp/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_LOGIN_FAILED')
    await expect(promise).rejects.toHaveProperty(['message'], 'Login failed (HTTP 403): Forbidden')
  })

  it('should throw when username is empty in classic login', async () => {
    const context = createMockContext({
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: '' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'a@b.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/mock/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_LOGIN_MISSING_CREDENTIALS')
    await expect(promise).rejects.toHaveProperty(['message'], 'Username, password, and email are all required')
  })

  it('should throw when classic login returns no token', async () => {
    const context = createMockContext({
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        if (url === 'https://example.org/-/user/org.couchdb.user:alice') {
          return createMockResponse({
            ok: true,
            status: 201,
            json: { ok: true },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'alice' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'alice@example.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/mock/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_LOGIN_NO_TOKEN')
    await expect(promise).rejects.toHaveProperty(['message'], 'The registry did not return an authentication token')
  })

  it('should throw when web login returns invalid response (missing loginUrl/doneUrl)', async () => {
    const context = createMockContext({
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: true,
            status: 200,
            json: { loginUrl: 'https://example.org/auth' },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
    })
    const opts = { configDir: '/mock/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_LOGIN_INVALID_RESPONSE')
    await expect(promise).rejects.toHaveProperty(['message'], 'The registry returned an invalid response for web-based login')
  })

  it('should fall back to classic login when web login returns 405', async () => {
    let savedSettings: Record<string, unknown> = {}
    const globalInfo = jest.fn()
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => ({}),
      writeIniFile: async (_configPath, settings) => {
        savedSettings = settings
      },
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 405,
            text: 'Method Not Allowed',
          })
        }
        if (url === 'https://example.org/-/user/org.couchdb.user:jane') {
          return createMockResponse({
            ok: true,
            status: 201,
            json: { ok: true, token: 'token-405' },
          })
        }
        throw new Error(`Unexpected call to fetch: ${url}`)
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'jane' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'jane@example.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/mock/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const result = await login({ context, opts })
    expect(result).toBe('Logged in on https://example.org/')
    expect(savedSettings).toMatchObject({
      '//example.org/:_authToken': 'token-405',
    })
    expect(globalInfo).toHaveBeenCalledWith('Logged in as jane')
  })

  it('should not trigger OTP for 401 without www-authenticate otp header', async () => {
    const context = createMockContext({
      readIniFile: async () => ({}),
      writeIniFile: async () => {},
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: false,
            status: 404,
            text: 'Not Found',
          })
        }
        // Return 401 but without www-authenticate: otp header
        return createMockResponse({
          ok: false,
          status: 401,
          text: 'Unauthorized',
          headers: { get: () => null },
        })
      },
      enquirer: {
        prompt: async (opts: { message: string, name: string, type: string }): Promise<Record<string, string>> => {
          if (opts.name === 'username') return { username: 'alice' }
          if (opts.name === 'password') return { password: 'pass' }
          if (opts.name === 'email') return { email: 'alice@example.com' }
          throw new Error(`Unexpected call to enquirer.prompt: ${opts.name}`)
        },
      },
    })
    const opts = { configDir: '/otp/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'ERR_PNPM_LOGIN_FAILED')
    await expect(promise).rejects.toHaveProperty(['message'], 'Login failed (HTTP 401): Unauthorized')
  })

  it('should succeed when config file does not exist (ENOENT)', async () => {
    let savedSettings: Record<string, unknown> = {}
    const globalInfo = jest.fn()
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => {
        throw Object.assign(new Error('ENOENT: no such file or directory'), { code: 'ENOENT' })
      },
      writeIniFile: async (_configPath, settings) => {
        savedSettings = settings
      },
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: true,
            status: 200,
            json: {
              loginUrl: 'https://example.org/auth/login',
              doneUrl: 'https://example.org/auth/done',
            },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'new-token' },
        })
      },
    })
    const opts = { configDir: '/nonexistent/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const result = await login({ context, opts })
    expect(result).toBe('Logged in on https://example.org/')
    expect(savedSettings).toMatchObject({
      '//example.org/:_authToken': 'new-token',
    })
    expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('https://example.org/auth/login'))
  })

  it('should propagate non-ENOENT errors from readIniFile', async () => {
    const globalInfo = jest.fn()
    const context = createMockContext({
      globalInfo,
      readIniFile: async () => {
        throw Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES' })
      },
      writeIniFile: async () => {},
      fetch: async url => {
        if (url === 'https://example.org/-/v1/login') {
          return createMockResponse({
            ok: true,
            status: 200,
            json: {
              loginUrl: 'https://example.org/auth/login',
              doneUrl: 'https://example.org/auth/done',
            },
          })
        }
        return createMockResponse({
          ok: true,
          status: 200,
          json: { token: 'tok' },
        })
      },
    })
    const opts = { configDir: '/broken/config', dir: '/mock', authConfig: {}, registry: 'https://example.org' }
    const promise = login({ context, opts })
    await expect(promise).rejects.toHaveProperty(['code'], 'EACCES')
    await expect(promise).rejects.toHaveProperty(['message'], 'EACCES: permission denied')
    expect(globalInfo.mock.calls).toEqual([
      [expect.stringContaining('https://example.org/auth/login')],
      ['Press ENTER to open the URL in your browser.'],
    ])
  })
})
