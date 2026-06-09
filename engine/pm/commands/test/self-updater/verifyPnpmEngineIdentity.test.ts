import crypto from 'node:crypto'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

const { verifyPnpmEngineIdentity } = await import('@pnpm/engine.pm.commands')

const REGISTRY = 'https://registry.example.test/'
const PNPM_INTEGRITY = 'sha512-pnpm-integrity'
const EXE_INTEGRITY = 'sha512-exe-integrity'

beforeEach(async () => {
  await setupMockAgent()
  process.env.PNPM_ENGINE_IDENTITY_REGISTRY = REGISTRY
})

afterEach(async () => {
  await teardownMockAgent()
  delete process.env.PNPM_ENGINE_IDENTITY_REGISTRY
})

describe('verifyPnpmEngineIdentity', () => {
  test('resolves when both pnpm and @pnpm/exe carry a valid registry signature over the installed bytes', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }])
    mockPackument('@pnpm/exe', EXE_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }])

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', {})).resolves.toBeUndefined()
  })

  test('throws when the installed bytes do not match what the registry signed (tamper)', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    // The registry signed the genuine integrity, but the lockfile pins a different one.
    mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', 'sha512-genuine-pnpm') }])
    mockPackument('@pnpm/exe', EXE_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', 'sha512-genuine-exe') }])

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', {})).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws when the engine version is absent from the trust-root registry', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/pnpm', method: 'GET' }).reply(404, {})
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/@pnpm%2Fexe', method: 'GET' }).reply(404, {}) // cspell:disable-line

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', {})).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('proceeds (no throw) when the trust root advertises no signing keys', async () => {
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/keys', method: 'GET' }).reply(200, { keys: [] })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', {})).resolves.toBeUndefined()
  })
})

function envLockfile (): EnvLockfile {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        configDependencies: {},
        packageManagerDependencies: {
          pnpm: { specifier: '9.1.0', version: '9.1.0' },
          '@pnpm/exe': { specifier: '9.1.0', version: '9.1.0' },
        },
      },
    },
    packages: {
      'pnpm@9.1.0': { resolution: { integrity: PNPM_INTEGRITY } },
      '@pnpm/exe@9.1.0': { resolution: { integrity: EXE_INTEGRITY } },
    },
    snapshots: {
      'pnpm@9.1.0': {},
      '@pnpm/exe@9.1.0': {},
    },
  } as unknown as EnvLockfile
}

function mockRegistryKey (key: ReturnType<typeof createSigningKey>): void {
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/keys', method: 'GET' })
    .reply(200, {
      keys: [{ expires: null, key: key.publicKey, keyid: key.keyid, keytype: 'ecdsa-sha2-nistp256', scheme: 'ecdsa-sha2-nistp256' }],
    }).persist()
}

function mockPackument (name: string, integrity: string, signatures: unknown): void {
  const encodedPath = name[0] === '@' ? `/${name.replace(/\//g, '%2F')}` : `/${name}`
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: encodedPath, method: 'GET' })
    .reply(200, {
      name,
      time: { '9.1.0': '2024-01-01T00:00:00.000Z' },
      versions: {
        '9.1.0': { name, version: '9.1.0', dist: { integrity, signatures, tarball: `${REGISTRY}${name}/-/x-9.1.0.tgz` } },
      },
    }).persist()
}

function createSigningKey (): { keyid: string, publicKey: string, sign: (id: string, integrity: string) => string } {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
  const publicKeyPem = publicKey.export({ format: 'pem', type: 'spki' }).toString()
  return {
    keyid: 'SHA256:test-key',
    publicKey: publicKeyPem.replace(/-----BEGIN PUBLIC KEY-----|-----END PUBLIC KEY-----|\s/g, ''),
    sign: (id, integrity) => {
      const signer = crypto.createSign('SHA256')
      signer.write(`${id}:${integrity}`)
      signer.end()
      return signer.sign(privateKey, 'base64')
    },
  }
}
