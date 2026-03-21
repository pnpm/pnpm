import { readFile } from 'node:fs/promises'

import { describe, expect, it } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { DEFAULT_CONTEXT, login, type LoginContext } from '../src/login.js'

const DEFAULT_OPTS = {
  configDir: '/tmp/test-config',
  dir: '/tmp',
  rawConfig: {},
  registry: 'https://registry.npmjs.org/',
}

const INTERACTIVE: Partial<LoginContext> = {
  process: { stdin: { isTTY: true }, stdout: { isTTY: true } },
}

describe('login', () => {
  it('should throw in non-interactive terminal', async () => {
    await expect(
      login({
        opts: DEFAULT_OPTS,
        context: {
          ...DEFAULT_CONTEXT,
          process: { stdin: { isTTY: false }, stdout: { isTTY: true } },
        },
      })
    ).rejects.toThrow('The login command requires an interactive terminal')
  })

  it('should use web login when registry supports it', async () => {
    const configDir = tempDir(false)

    const result = await login({
      opts: { ...DEFAULT_OPTS, configDir },
      context: {
        ...DEFAULT_CONTEXT,
        ...INTERACTIVE,
        setTimeout: (cb) => {
          cb()
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
  })

  it('should fall back to classic login when web login returns 404', async () => {
    const configDir = tempDir(false)

    const result = await login({
      opts: { ...DEFAULT_OPTS, configDir },
      context: {
        ...DEFAULT_CONTEXT,
        ...INTERACTIVE,
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

    const rcContent = await readFile(`${configDir}/rc`, 'utf8')
    expect(rcContent).toContain('_authToken=classic-token-456')
  })
})
