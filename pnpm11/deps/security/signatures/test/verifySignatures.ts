import crypto from 'node:crypto'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import { getNpmSigningKeys, verifyInstalledPackageSignatures, verifySignatures } from '@pnpm/deps.security.signatures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

const REGISTRY = 'https://registry.example.test/'
const SECOND_REGISTRY = 'https://second-registry.example.test/'
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

  test('reports malformed signatures metadata as invalid', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument({ signatures: 'not-an-array' })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(0)
    expect(result.missing).toEqual([])
    expect(result.invalid).toEqual([{
      name: 'signed-pkg',
      registry: REGISTRY,
      version: '1.0.0',
      integrity: INTEGRITY,
      resolved: `${REGISTRY}signed-pkg/-/signed-pkg-1.0.0.tgz`,
      reason: 'Malformed registry signatures metadata for signed-pkg@1.0.0',
    }])
  })

  test('reports malformed registry keys as invalid signatures instead of crashing', async () => {
    const key = createSigningKey()
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/keys', method: 'GET' })
      .reply(200, {
        keys: [{
          expires: null,
          key: 'not-actually-base64-pem-content',
          keyid: key.keyid,
          keytype: 'ecdsa-sha2-nistp256',
          scheme: 'ecdsa-sha2-nistp256',
        }],
      })
    mockPackument({
      signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }],
    })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(0)
    expect(result.missing).toEqual([])
    expect(result.invalid).toHaveLength(1)
    expect(result.invalid[0].reason).toBe(`signed-pkg@1.0.0 has an invalid registry signature with keyid ${key.keyid}`)
  })

  test('reports signatures with unknown keys', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument({ signatures: [{ keyid: 'SHA256:unknown-key', sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }] })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(0)
    expect(result.missing).toEqual([])
    expect(result.invalid).toHaveLength(1)
    expect(result.invalid[0].reason).toBe('signed-pkg@1.0.0 has a registry signature with keyid SHA256:unknown-key but no corresponding public key can be found')
  })

  test('accepts a package when one valid signature is present alongside a junk one', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    // A mirror could append a junk signature; a valid trusted one must still pass.
    mockPackument({
      signatures: [
        { keyid: 'SHA256:junk-key', sig: 'not-a-real-signature' },
        { keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) },
      ],
    })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result).toMatchObject({ audited: 1, invalid: [], missing: [], verified: 1 })
  })

  test('skips registries without signing keys', async () => {
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/keys', method: 'GET' })
      .reply(404, {})

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result).toEqual({
      audited: 0,
      invalid: [],
      missing: [],
      verified: 0,
    })
  })

  test('skips packages missing from the registry packument', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/signed-pkg', method: 'GET' })
      .reply(404, {})

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result).toEqual({
      audited: 0,
      invalid: [],
      missing: [],
      verified: 0,
    })
  })

  test('audits packages from registries that provide signing keys', async () => {
    const key = createSigningKey()
    mockRegistryKey(key)
    mockPackument({ signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }] })
    getMockAgent().get(SECOND_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/keys', method: 'GET' })
      .reply(404, {})

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
      { name: 'unsigned-registry-pkg', registry: SECOND_REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result).toEqual({
      audited: 1,
      invalid: [],
      missing: [],
      verified: 1,
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

  test('does not reject expired keys when publish time is missing', async () => {
    const key = createSigningKey({ expires: '2024-01-01T00:00:00.000Z' })
    mockRegistryKey(key)
    mockPackument({ signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }], time: null })

    const result = await verifySignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0' },
    ], () => undefined, {})

    expect(result.verified).toBe(1)
    expect(result.invalid).toEqual([])
  })
})

describe('verifyInstalledPackageSignatures', () => {
  beforeEach(async () => {
    await setupMockAgent()
  })

  afterEach(async () => {
    await teardownMockAgent()
  })

  test('verifies when the installed integrity matches what the registry signed', async () => {
    const key = createSigningKey()
    mockPackument({ signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }] })

    const result = await verifyInstalledPackageSignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: INTEGRITY },
    ], [toRegistryKey(key)], () => undefined, {})

    expect(result).toEqual({ verified: true, failures: [] })
  })

  test('fails when the installed bytes differ from what the registry signed (tamper)', async () => {
    const key = createSigningKey()
    // The registry signed INTEGRITY, but a tampered lockfile claims a different one.
    mockPackument({ signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) }] })

    const result = await verifyInstalledPackageSignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: 'sha512-tampered-bytes' },
    ], [toRegistryKey(key)], () => undefined, {})

    expect(result.verified).toBe(false)
    expect(result.failures).toHaveLength(1)
    expect(result.failures[0]).toMatchObject({ name: 'signed-pkg', version: '1.0.0', category: 'invalid' })
    expect(result.failures[0].reason).toContain('invalid registry signature')
  })

  test('fails when the signature was made by a key that is not trusted', async () => {
    const signingKey = createSigningKey()
    const trustedButDifferentKey = createSigningKey()
    mockPackument({ signatures: [{ keyid: signingKey.keyid, sig: signingKey.sign('signed-pkg@1.0.0', INTEGRITY) }] })

    const result = await verifyInstalledPackageSignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: INTEGRITY },
    ], [{ ...toRegistryKey(trustedButDifferentKey), keyid: 'SHA256:not-the-signing-key' }], () => undefined, {})

    expect(result.verified).toBe(false)
    expect(result.failures[0]).toMatchObject({ category: 'invalid' })
  })

  test('verifies when a valid trusted signature is present alongside an untrusted/junk one', async () => {
    const key = createSigningKey()
    // A malicious mirror cannot force a failure by appending a junk signature.
    mockPackument({
      signatures: [
        { keyid: 'SHA256:junk-key', sig: 'not-a-real-signature' },
        { keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', INTEGRITY) },
      ],
    })

    const result = await verifyInstalledPackageSignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: INTEGRITY },
    ], [toRegistryKey(key)], () => undefined, {})

    expect(result).toEqual({ verified: true, failures: [] })
  })

  test('fails when the registry has no signature for the package', async () => {
    const key = createSigningKey()
    mockPackument({ signatures: [] })

    const result = await verifyInstalledPackageSignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: INTEGRITY },
    ], [toRegistryKey(key)], () => undefined, {})

    expect(result.verified).toBe(false)
    expect(result.failures[0]).toMatchObject({ category: 'absent' })
    expect(result.failures[0].reason).toContain('no registry signature')
  })

  test('fails when the package is not published on the registry', async () => {
    const key = createSigningKey()
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/signed-pkg', method: 'GET' })
      .reply(404, {})

    const result = await verifyInstalledPackageSignatures([
      { name: 'signed-pkg', registry: REGISTRY, version: '1.0.0', integrity: INTEGRITY },
    ], [toRegistryKey(key)], () => undefined, {})

    expect(result.verified).toBe(false)
    expect(result.failures[0]).toMatchObject({ category: 'absent' })
    expect(result.failures[0].reason).toContain('not published')
  })
})

function toRegistryKey (key: ReturnType<typeof createSigningKey>) {
  return { expires: null, key: key.publicKey, keyid: key.keyid, keytype: 'ecdsa-sha2-nistp256', scheme: 'ecdsa-sha2-nistp256' }
}

describe('getNpmSigningKeys', () => {
  test('returns npm\'s embedded public signing keys', () => {
    const keys = getNpmSigningKeys()
    expect(keys.some((k) => k.keyid === 'SHA256:DhQ8wR5APBvFHLF/+Tc+AYvPOdTpcIDqOhxsBHRwC7U')).toBe(true)
    expect(keys.every((k) => k.keytype === 'ecdsa-sha2-nistp256')).toBe(true)
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

function mockPackument (opts: { signatures: unknown, time?: string | null }): void {
  const time = Object.hasOwn(opts, 'time')
    ? (opts.time == null ? {} : { '1.0.0': opts.time })
    : { '1.0.0': '2023-01-01T00:00:00.000Z' }
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/signed-pkg', method: 'GET' })
    .reply(200, {
      name: 'signed-pkg',
      time,
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
