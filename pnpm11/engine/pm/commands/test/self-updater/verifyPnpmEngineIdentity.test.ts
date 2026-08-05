import crypto from 'node:crypto'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { familySync } from 'detect-libc'

const { exePlatformPkgDirName, exePlatformPkgDirNameNext, verifyPnpmEngineIdentity } = await import('@pnpm/engine.pm.commands')

const REGISTRY = 'https://registry.example.test/'
const PNPM_INTEGRITY = 'sha512-pnpm-integrity'
const EXE_INTEGRITY = 'sha512-exe-integrity'
const PLATFORM_INTEGRITY = 'sha512-platform-integrity'
const PLATFORM_PKG_NAME = `@pnpm/${exePlatformPkgDirName(process.platform, process.arch, familySync())}`
const PLATFORM_PKG_NAME_NEXT = `@pnpm/${exePlatformPkgDirNameNext(process.platform, process.arch, familySync())}`

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

describe('verifyPnpmEngineIdentity', () => {
  test('resolves when both pnpm and @pnpm/exe carry a valid registry signature over the installed bytes', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, PLATFORM_INTEGRITY) }] })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
  })

  test('throws when the installed bytes do not match what the registry signed (tamper)', async () => {
    const key = createSigningKey()
    // The registry signed the genuine integrity, but the lockfile pins a different one.
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', 'sha512-genuine-pnpm') }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', 'sha512-genuine-exe') }] })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws when the engine is signed by a key pnpm does not trust', async () => {
    const signingKey = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: signingKey.keyid, sig: signingKey.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: signingKey.keyid, sig: signingKey.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })

    // Trust a different key than the one that signed.
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws when the engine version is absent from the registry', async () => {
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/pnpm', method: 'GET' }).reply(404, {})
    getMockAgent().get(REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/@pnpm%2Fexe', method: 'GET' }).reply(404, {}) // cspell:disable-line

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws (fails closed) when the registry is unreachable', async () => {
    // No intercept registered and net connect disabled, so the packument fetch fails.
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('skips (no throw) when no trusted keys are provided', async () => {
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', { registries: { default: REGISTRY }, trustedKeys: [] })).resolves.toBeUndefined()
  })

  test('throws when an engine component in the lockfile has no integrity metadata', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })

    const lockfile = envLockfile()
    ;(lockfile.packages as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { resolution: { tarball: `${REGISTRY}@pnpm/exe/-/exe-9.1.0.tgz` } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/integrity metadata is missing/)
  })

  test('throws when the platform binary in the lockfile has no integrity metadata', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })

    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { optionalDependencies: { [PLATFORM_PKG_NAME]: '9.1.0' } }
    ;(lockfile.packages as Record<string, unknown>)[`${PLATFORM_PKG_NAME}@9.1.0`] = { resolution: { tarball: `${REGISTRY}${PLATFORM_PKG_NAME}/-/x-9.1.0.tgz` } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/integrity metadata is missing/)
  })

  test('resolves when the platform binary carries a valid registry signature', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, PLATFORM_INTEGRITY) }] })

    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { optionalDependencies: { [PLATFORM_PKG_NAME]: '9.1.0' } }
    ;(lockfile.packages as Record<string, unknown>)[`${PLATFORM_PKG_NAME}@9.1.0`] = { resolution: { integrity: PLATFORM_INTEGRITY } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
  })

  test('throws when the wrapper snapshot lists no platform binaries', async () => {
    const key = createSigningKey()
    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = {}

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/platform binaries are missing/)
  })

  test('throws when no platform binary in the lockfile matches the host', async () => {
    const key = createSigningKey()
    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { optionalDependencies: { '@pnpm/exe.aix-mips': '9.1.0' } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/native binary: it is missing/)
  })

  test('verifies the platform binary of the unscoped pnpm, which is the native wrapper from v12', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@12.0.0', PNPM_INTEGRITY) }], version: '12.0.0' })
    mockPackument({ name: PLATFORM_PKG_NAME_NEXT, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME_NEXT}@12.0.0`, 'sha512-genuine-platform') }], version: '12.0.0' })

    await expect(verifyPnpmEngineIdentity(envLockfileV12(), '12.0.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
  })
})

/** An env lockfile of a v12 engine, whose only package-manager dependency is the native `pnpm`. */
function envLockfileV12 (): EnvLockfile {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        configDependencies: {},
        packageManagerDependencies: {
          pnpm: { specifier: '12.0.0', version: '12.0.0' },
        },
      },
    },
    packages: {
      'pnpm@12.0.0': { resolution: { integrity: PNPM_INTEGRITY } },
      [`${PLATFORM_PKG_NAME_NEXT}@12.0.0`]: { resolution: { integrity: PLATFORM_INTEGRITY } },
    },
    snapshots: {
      'pnpm@12.0.0': { optionalDependencies: { [PLATFORM_PKG_NAME_NEXT]: '12.0.0' } },
    },
  } as unknown as EnvLockfile
}

function optsTrusting (key: ReturnType<typeof createSigningKey>) {
  return {
    registries: { default: REGISTRY },
    trustedKeys: [{ expires: null, key: key.publicKey, keyid: key.keyid, keytype: 'ecdsa-sha2-nistp256', scheme: 'ecdsa-sha2-nistp256' }],
  }
}

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
      [`${PLATFORM_PKG_NAME}@9.1.0`]: { resolution: { integrity: PLATFORM_INTEGRITY } },
    },
    snapshots: {
      'pnpm@9.1.0': {},
      '@pnpm/exe@9.1.0': { optionalDependencies: { [PLATFORM_PKG_NAME]: '9.1.0' } },
    },
  } as unknown as EnvLockfile
}

function mockPackument ({ name, integrity, signatures, version = '9.1.0' }: { name: string, integrity: string, signatures: unknown, version?: string }): void {
  const encodedPath = name[0] === '@' ? `/${name.replace(/\//g, '%2F')}` : `/${name}`
  getMockAgent().get(REGISTRY.replace(/\/$/, ''))
    .intercept({ path: encodedPath, method: 'GET' })
    .reply(200, {
      name,
      time: { [version]: '2024-01-01T00:00:00.000Z' },
      versions: {
        [version]: { name, version, dist: { integrity, signatures, tarball: `${REGISTRY}${name}/-/x-${version}.tgz` } },
      },
    }).persist()
}

function createSigningKey (): { keyid: string, publicKey: string, sign: (id: string, integrity: string) => string } {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
  const publicKeyPem = publicKey.export({ format: 'pem', type: 'spki' }).toString()
  return {
    keyid: `SHA256:test-key-${crypto.randomBytes(4).toString('hex')}`,
    publicKey: publicKeyPem.replace(/-----BEGIN PUBLIC KEY-----|-----END PUBLIC KEY-----|\s/g, ''),
    sign: (id, integrity) => {
      const signer = crypto.createSign('SHA256')
      signer.write(`${id}:${integrity}`)
      signer.end()
      return signer.sign(privateKey, 'base64')
    },
  }
}
