import { describe, expect, it } from '@jest/globals'

import { login, type LoginContext } from '../src/login.js'

function createMockContext (overrides: Partial<LoginContext> = {}): LoginContext {
  return {
    Date: { now: () => 0 },
    setTimeout: (cb) => {
      cb()
    },
    fetch: async () => ({ ok: false, status: 500, json: async () => ({}), text: async () => '' }),
    prompt: async () => ({}),
    process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
    ...overrides,
  }
}

describe('login', () => {
  it('should throw in non-interactive terminal', async () => {
    const context = createMockContext({
      process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
    })

    await expect(
      login(
        {
          configDir: '/tmp/test-config',
          dir: '/tmp',
          rawConfig: {},
          userAgent: 'pnpm',
          registry: 'https://registry.npmjs.org/',
        },
        context
      )
    ).rejects.toThrow('The login command requires an interactive terminal')
  })

  it('should use web login when registry supports it', async () => {
    const tmpDir = `/tmp/pnpm-login-test-${Date.now()}`

    const context = createMockContext({
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
        return { ok: false, status: 404, json: async () => ({}), text: async () => '' }
      },
    })

    const result = await login(
      {
        configDir: tmpDir,
        dir: '/tmp',
        rawConfig: {},
        userAgent: 'pnpm',
        registry: 'https://registry.npmjs.org/',
      },
      context
    )

    expect(result).toContain('Logged in')

    // Clean up
    const { rm } = await import('node:fs/promises')
    await rm(tmpDir, { recursive: true, force: true })
  })

  it('should fall back to classic login when web login returns 404', async () => {
    const tmpDir = `/tmp/pnpm-login-test-${Date.now()}`

    const context = createMockContext({
      fetch: async (url: string) => {
        if (url.includes('/-/v1/login')) {
          return {
            ok: false,
            status: 404,
            json: async () => ({}),
            text: async () => 'Not Found',
          }
        }
        if (url.includes('org.couchdb.user')) {
          return {
            ok: true,
            status: 201,
            json: async () => ({ ok: true, token: 'classic-token-456' }),
            text: async () => '',
          }
        }
        return { ok: false, status: 500, json: async () => ({}), text: async () => '' }
      },
      prompt: async (opts: { message: string, name: string, type: string }) => {
        if (opts.name === 'username') return { username: 'john' }
        if (opts.name === 'password') return { password: 'secret' }
        if (opts.name === 'email') return { email: 'john@example.com' }
        return {}
      },
    })

    const result = await login(
      {
        configDir: tmpDir,
        dir: '/tmp',
        rawConfig: {},
        userAgent: 'pnpm',
        registry: 'https://registry.npmjs.org/',
      },
      context
    )

    expect(result).toContain('Logged in')

    // Verify token was saved
    const { readFile } = await import('node:fs/promises')
    const rcContent = await readFile(`${tmpDir}/rc`, 'utf8')
    expect(rcContent).toContain('_authToken=classic-token-456')

    // Clean up
    const { rm } = await import('node:fs/promises')
    await rm(tmpDir, { recursive: true, force: true })
  })
})
