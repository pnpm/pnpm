import crypto from 'node:crypto'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import { verifySignatures } from '@pnpm/deps.compliance.audit'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

const REGISTRY = 'https://registry.example.test/'
const INTEGRITY = 'sha512-test-integrity'

describe('verifySignatures', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  test('verifies registry signatures', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument({
      signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }],
    })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result).toEqual({
      audited: 1,
      invalid: [],
      missing: [],
      verified: 1,
    })
  })

  test('reports missing signatures when registry provides signing keys', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument({ signatures: [] })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(0)
    expect(result.invalid).toEqual([])
    expect(result.missing).toEqual([{ name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: INTEGRITY, resolved: `${REGISTRY}signed-pkg/-/signed-pkg-1.0.0.tgz` }])
  })

  test('reports invalid signatures', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument({
      signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', 'different-integrity') }],
    })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(0)
    expect(result.missing).toEqual([])
    expect(result.invalid).toHaveLength(1)
    expect(result.invalid[0]).toMatchObject({
      name: 'signed-pkg',
      registry: REGISTRY,
      version: '1.0.0',
      integrity: INTEGRITY,
      reason: 'signed-pkg@1.0.0 has an invalid registry signature with keyid SHA256:test-key',
    })
  })

  test('reports signatures with expired keys', async () => {
    const key = createSigningKey({ expires: '2024-01-01T00:00:00.000Z' })
    mockRegistryKey(key)
    mockPackument({
      signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }],
      time: '2024-02-01T00:00:00.000Z',
    })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(0)
    expect(result.invalid).toHaveLength(1)
    expect(result.invalid[0].reason).toBe('signed-pkg@1.0.0 has a registry signature with keyid SHA256:test-key but the corresponding public key has expired 2024-01-01T00:00:00.000Z')
  })
})

function mockRegistryKey (key: ReturnType<typeof createSigningKey>): void {
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/keys', method: 'GET' })
    .reply(200, {
      keys: [{
        expires: key.expires,
        key: key.publicKey,
        keyid: key.keyid,
        keytype: 'ecdsa-sha2-nistp256',
        scheme: 'ecdsa-sha2-nistp256',
      }],
    })
}

function mockPackument (opts: { signatures: Array<{ keyid: string, sig: string }>, time?: string }): void {
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/signed-pkg', method: 'GET' })
    .reply(200, {
      name: 'signed-pkg',
      time: {
        '1.0.0': opts.time ?? '2023-01-01T00:00:00.000Z',
      },
      versions: {
        '1.0.0': {
          dist: {
            integrity: INTEGRITY,
            shasum: 'test-shasum',
            signatures: opts.signatures,
            tarball: `${REGISTRY}signed-pkg/-/signed-pkg-1.0.0.tgz`,
          },
          name: 'signed-pkg',
          version: '1.0.0',
        },
      },
    })
}

function createSigningKey (opts?: { expires?: string | null }): {
  expires: string | null
  keyid: string
  publicKey: string
  sign: (id: string, integrity: string) => string
} {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
  const publicKeyPem = publicKey.export({ format: 'pem', type: 'spki' }).toString()
  return {
    expires: opts?.expires ?? null,
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
