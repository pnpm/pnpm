import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { unpublish } from '@pnpm/registry-access.commands'
import { publish } from '@pnpm/releasing.commands'
import { DEFAULT_OPTS as BASE_OPTS, overrideTty } from '@pnpm/testing.command-defaults'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { getRegistryMockToken, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { safeExeca as execa } from 'execa'

const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
}

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    '@': { authToken: getRegistryMockToken() },
  },
}

async function getVersions (pkgName: string): Promise<string[]> {
  try {
    const { stdout } = await execa('pnpm', [
      'view',
      pkgName,
      'versions',
      '--json',
      '--registry',
      REGISTRY,
    ])
    const parsed = JSON.parse(stdout?.toString() ?? '[]')
    if (typeof parsed === 'string') return [parsed]
    return parsed
  } catch {
    return []
  }
}

async function publishVersion (name: string, version: string): Promise<void> {
  prepare({
    name,
    version,
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
  }, [])
}

test('unpublish: should unpublish a specific version', async () => {
  const pkgName = 'test-unpublish-version'
  await publishVersion(pkgName, '0.0.1')
  await publishVersion(pkgName, '0.0.2')

  const result = await unpublish.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, [`${pkgName}@0.0.1`])

  expect(result).toContain('Successfully unpublished')
  expect(result).toContain('1 version(s)')

  const versions = await getVersions(pkgName)
  expect(versions).not.toContain('0.0.1')
  expect(versions).toContain('0.0.2')
})

test('unpublish: should unpublish entire package with --force', async () => {
  const pkgName = 'test-unpublish-force'
  await publishVersion(pkgName, '0.0.1')

  const result = await unpublish.handler({
    ...DEFAULT_OPTS,
    cliOptions: { force: true },
    configByUri: CONFIG_BY_URI,
  }, [pkgName])

  expect(result).toContain('Successfully unpublished')

  const versions = await getVersions(pkgName)
  expect(versions).toEqual([])
})

test('unpublish: should refuse to unpublish entire package without --force', async () => {
  const pkgName = 'test-unpublish-no-force'
  await publishVersion(pkgName, '0.0.1')

  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, [pkgName])
  }).rejects.toThrow('pnpm unpublish --force')
})

test('unpublish: should throw error when package not found', async () => {
  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['nonexistent-package-99999'])
  }).rejects.toThrow('Package "nonexistent-package-99999" not found in registry')
})

test('unpublish: should throw error when no package name provided', async () => {
  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, [])
  }).rejects.toThrow('Package name is required')
})

test('unpublish: should throw error when version not found', async () => {
  const pkgName = 'test-unpublish-no-ver'
  await publishVersion(pkgName, '0.0.1')

  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, [`${pkgName}@9.9.9`])
  }).rejects.toThrow('No versions match')
})

describe('unpublish: OTP challenges', () => {
  const MOCK_REGISTRY = 'https://unpublish.example'
  // Only registry options: the TLS/proxy settings in DEFAULT_OPTS would build
  // a private dispatcher that bypasses the mock agent.
  const OPTS = {
    configByUri: {},
    registriesByScope: { default: `${MOCK_REGISTRY}/` },
  }
  const PACKUMENT = {
    name: 'test-pkg',
    _rev: '3-abc',
    'dist-tags': { latest: '0.0.2' },
    versions: {
      '0.0.1': { name: 'test-pkg', version: '0.0.1', dist: { tarball: `${MOCK_REGISTRY}/test-pkg/-/test-pkg-0.0.1.tgz` } },
      '0.0.2': { name: 'test-pkg', version: '0.0.2', dist: { tarball: `${MOCK_REGISTRY}/test-pkg/-/test-pkg-0.0.2.tgz` } },
    },
  }
  const WEB_AUTH_CHALLENGE = {
    error: 'one-time pass required',
    authUrl: 'https://auth.example/login',
    doneUrl: `${MOCK_REGISTRY}/-/v1/done`,
  }
  type Headers = Record<string, string | string[] | undefined>

  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  test('a 401 carrying authUrl/doneUrl is an OTP challenge, not a missing login', async () => {
    const restoreTty = overrideTty(false)
    try {
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'GET', path: '/test-pkg' }).reply(200, PACKUMENT)
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'DELETE', path: '/test-pkg/-rev/3-abc' }).reply(401, WEB_AUTH_CHALLENGE)

      await expect(unpublish.handler({ ...OPTS, cliOptions: { force: true } }, ['test-pkg']))
        .rejects.toMatchObject({ code: 'ERR_PNPM_OTP_NON_INTERACTIVE', authUrl: 'https://auth.example/login' })
    } finally {
      restoreTty()
    }
  })

  test('the classic "one-time pass" wording on the packument PUT is an OTP challenge too', async () => {
    const restoreTty = overrideTty(false)
    try {
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'GET', path: '/test-pkg' }).reply(200, PACKUMENT)
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'PUT', path: '/test-pkg/-rev/3-abc' })
        .reply(401, { error: 'You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA.' })

      await expect(unpublish.handler({ ...OPTS, cliOptions: {} }, ['test-pkg@0.0.1']))
        .rejects.toMatchObject({ code: 'ERR_PNPM_OTP_NON_INTERACTIVE' })
    } finally {
      restoreTty()
    }
  })

  test('a plain 401 is reported as a missing login, with its body stripped of terminal control characters', async () => {
    getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'GET', path: '/test-pkg' }).reply(200, PACKUMENT)
    getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'DELETE', path: '/test-pkg/-rev/3-abc' }).reply(401, 'unauthorized \u001b[2J spoofed\r line')

    await expect(unpublish.handler({ ...OPTS, cliOptions: { force: true } }, ['test-pkg']))
      .rejects.toMatchObject({ code: 'ERR_PNPM_UNAUTHORIZED', message: 'You must be logged in to unpublish packages. unauthorized [2J spoofed line' })
  })

  test('--otp sends the code up front under the legacy auth type', async () => {
    let deleteHeaders: Headers = {}
    getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'GET', path: '/test-pkg' }).reply(200, PACKUMENT)
    getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'DELETE', path: '/test-pkg/-rev/3-abc' }).reply(({ headers }) => {
      deleteHeaders = headers as Headers
      return { statusCode: 200, data: {} }
    })

    await expect(unpublish.handler({ ...OPTS, cliOptions: { force: true, otp: '123456' } }, ['test-pkg']))
      .resolves.toBe('Successfully unpublished all 2 version(s) of test-pkg')
    expect(deleteHeaders['npm-auth-type']).toBe('legacy')
    expect(deleteHeaders['npm-otp']).toBe('123456')
  })

  test('the web-auth flow answers the challenge and its token is reused by the tarball delete', async () => {
    const restoreTty = overrideTty(true)
    const putOtpHeaders: Array<string | string[] | undefined> = []
    let tarballDeleteHeaders: Headers = {}
    try {
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'GET', path: '/test-pkg' }).reply(200, PACKUMENT).times(2)
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'PUT', path: '/test-pkg/-rev/3-abc' }).reply(({ headers }) => {
        const otp = (headers as Headers)['npm-otp']
        putOtpHeaders.push(otp)
        if (otp === 'web-token') return { statusCode: 200, data: {} }
        return { statusCode: 401, data: WEB_AUTH_CHALLENGE }
      }).times(2)
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'GET', path: '/-/v1/done' }).reply(200, { token: 'web-token' })
      getMockAgent().get(MOCK_REGISTRY).intercept({ method: 'DELETE', path: '/test-pkg/-/test-pkg-0.0.1.tgz/-rev/3-abc' }).reply(({ headers }) => {
        tarballDeleteHeaders = headers as Headers
        return { statusCode: 200, data: {} }
      })

      await expect(unpublish.handler({ ...OPTS, cliOptions: {} }, ['test-pkg@0.0.1']))
        .resolves.toBe('Successfully unpublished 1 version(s) of test-pkg')
      expect(putOtpHeaders).toEqual([undefined, 'web-token'])
      expect(tarballDeleteHeaders['npm-auth-type']).toBe('web')
      expect(tarballDeleteHeaders['npm-otp']).toBe('web-token')
    } finally {
      restoreTty()
    }
  })
})
